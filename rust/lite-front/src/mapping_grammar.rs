// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `parser.MappingProtocolParser`, the `###Mapping` grammar (docs/rust-port/PHASE2.md §4 step 4), on the
//! cursor `mapping_parser` defines: `parseMapping` (includes and their store substitutions, the body's ordered
//! sections), each class mapping kind (relational -- its `~filter`/`~distinct`/`~groupBy`/`~primaryKey`/`~mainTable`
//! prefix, property lines, embedded, otherwise-embedded and inline properties, `scope(...)` blocks --, Pure
//! model-to-model with `~src` and `~filter`, the clean-sheet function form, AggregationAware, Operation and merge,
//! Relation (`~func`/`~src`), ServiceStore and its legacy tail (`ServiceLegacyMappingParser`), MongoDB), association
//! mappings (relational, XStore, ModelJoin, clean-sheet), and enumeration mappings. The tests a mapping carries are
//! `mapping_tests`'.
//!
//! Relational operations are the Database grammar's (`database_parser::operation_at`); every Pure expression is the
//! expression parser's, over a slice of the shared stream (its spans the file's), or -- where the engine re-parses
//! `getText()` at an anchor -- over a padded re-lex of the same text, as Java does.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, is_identifier_token, strip_double_quotes, throw_at, ty_or_slack, unquote_path_iae};
use crate::database_parser::{ScopeCtx, operation_at, rel_op_span, scoped_operation_at, with_rel_op_span};
use crate::doors::{Dialect, JavaError};
use crate::jcat;
use crate::jtypes::{JStr, JValue};
use crate::lexer::{TokenStream, TokenType};
use crate::mapping_parser::MappingCursor;
use crate::nodes::{Span, boxed};
use crate::records::{
    LambdaFunction, Multiplicity, PAggregateSetImplementation, PAggregateValue, PAssociationMapping, PBodyBuildInfo,
    PClassMapping, PClassMappingAggregationAware, PClassMappingFunction, PClassMappingMergeOperation,
    PClassMappingMongoDb, PClassMappingOperation, PClassMappingPure, PClassMappingRel, PClassMappingRelation,
    PEmbeddedPropertyMapping, PEnumSourceValue, PEnumValueMapping, PEnumerationMapping, PFilterMapping,
    PFunctionAssociationMapping, PInlineEmbeddedPropertyMapping, PJoinPtr, PLocalProp, PMapping, PMappingInclude,
    PModelJoinAssociationMapping, POtherwiseEmbeddedPropertyMapping, PParameterBuildInfo, PParametersBuildInfo,
    PPathOffset, PPointer, PPropertyMapping, PPurePropertyMapping, PRelAssocPropertyMapping, PRelAssociationMapping,
    PRelOp, PRelPropertyMapping, PRelationFnPropertyMapping, PRelationSrcLambda, PRequestBuildInfo, PServiceMapping,
    PServicePtr, PServiceSegment, PServiceStoreClassMapping, PServiceStoreLocalProp, PStoreSubstitution, PTablePtr,
    PXStoreAssociationMapping, PXStorePropertyMapping, SourceInfo, ValueSpecification, Variable,
};
use crate::spec_parser::{parse_code_block, parse_code_block_tokens, parse_tokens};
use crate::ustr::{eq, index_of_units, last_index_of_str, u};

use TokenType as T;

/// `MappingProtocolParser.parse(ts, tokenIndex, sectionStartLine, endOut, dialect)`: one `Mapping qn ( ... )` at
/// `token_index`, the mapping section's first content line given (-1 unknown), and the token where it ended.
pub fn parse_mapping(
    ts: &TokenStream<'_>,
    token_index: usize,
    section_start_line: i64,
    dialect: Dialect,
) -> R<(PMapping, usize)> {
    let mut p = MappingCursor::at(ts, token_index, dialect);
    p.section_start_line = section_start_line;
    let m = p.parse_mapping()?;
    Ok((m, p.pos))
}

// A relational class mapping's ~clauses: a fixed, strictly ascending prefix, each at most once, before the properties.
const RANK_FILTER: i32 = 1;
const RANK_DISTINCT: i32 = 2;
const RANK_GROUP_BY: i32 = 3;
const RANK_PRIMARY_KEY: i32 = 4;
const RANK_MAIN_TABLE: i32 = 5;
const RANK_PROPERTIES: i32 = 6;

// The mapping body's sections, in the one order the grammar allows; the first two repeat.
const BODY_INCLUDE: i32 = 1;
const BODY_ELEMENT: i32 = 2;
const BODY_LEGACY_TESTS: i32 = 3;
const BODY_TEST_SUITES: i32 = 4;

/// `MappingOperation.byFunction`: the router functions an Operation class mapping names, by their exact path -- the
/// discriminator, or none for MERGE and an unknown function.
fn operation_discriminator(fqn: &[u16]) -> Option<&'static str> {
    const OPS: [(&str, &str); 4] = [
        (
            "ROUTER_UNION",
            "meta::pure::router::operations::special_union_OperationSetImplementation_1__SetImplementation_MANY_",
        ),
        (
            "STORE_UNION",
            "meta::pure::router::operations::union_OperationSetImplementation_1__SetImplementation_MANY_",
        ),
        (
            "INHERITANCE",
            "meta::pure::router::operations::inheritance_OperationSetImplementation_1__SetImplementation_MANY_",
        ),
        (
            "MERGE",
            "meta::pure::router::operations::merge_OperationSetImplementation_1__SetImplementation_MANY_",
        ),
    ];
    let op = OPS.iter().find(|(_, f)| eq(fqn, f))?.0;
    if op == "MERGE" { None } else { Some(op) }
}

/// `SetIds.of(declared, fqn)`: the declared id, else the class path with `::` spelled `_`.
fn set_id_of(declared: Option<&JStr>, fqn: &[u16]) -> Vec<u16> {
    match declared {
        Some(d) if !d.0.is_empty() => d.0.clone(),
        _ => {
            let mut out = Vec::with_capacity(fqn.len());
            let mut i = 0;
            while i < fqn.len() {
                if i + 1 < fqn.len() && fqn[i] == u16::from(b':') && fqn[i + 1] == u16::from(b':') {
                    out.push(u16::from(b'_'));
                    i += 2;
                } else {
                    out.push(fqn[i]);
                    i += 1;
                }
            }
            out
        }
    }
}

/// `shiftLines(si, delta)`: both lines moved by `delta`.
pub(crate) fn shift_lines(si: &SourceInfo, delta: i64) -> SourceInfo {
    SourceInfo {
        source_id: si.source_id.clone(),
        start_line: (i64::from(si.start_line) + delta) as i32,
        start_column: si.start_column,
        end_line: (i64::from(si.end_line) + delta) as i32,
        end_column: si.end_column,
    }
}

fn split_last(qn: &[u16]) -> (Vec<u16>, Vec<u16>) {
    match last_index_of_str(qn, "::") {
        None => (Vec::new(), qn.to_vec()),
        Some(cut) => (qn[..cut].to_vec(), qn[cut + 2..].to_vec()),
    }
}

fn pointer(kind: &str, path: &[u16], span: &SourceInfo) -> Box<PPointer> {
    Box::new(PPointer {
        r#type: JStr::from(kind),
        path: JStr(path.to_vec()),
        source_information: boxed(span.clone()),
    })
}

fn is_open(t: TokenType) -> bool {
    matches!(t, T::PAREN_OPEN | T::BRACKET_OPEN | T::BRACE_OPEN)
}

fn is_close(t: TokenType) -> bool {
    matches!(t, T::PAREN_CLOSE | T::BRACKET_CLOSE | T::BRACE_CLOSE)
}

/// A Relation binding's right-hand side (`MappingProtocolParser.RelationBinding`): a column name, or a row
/// expression richer than one; the span its tokens.
struct RelationBinding {
    column: Option<JStr>,
    expr: Option<ValueSpecification>,
    span: SourceInfo,
}

/// `exprWrapper(rb, braceLine)`: an expression binding's valueFn wrapper span, its lines shifted down by the RHS line
/// less the class mapping's brace line.
fn expr_wrapper(rb: &RelationBinding, brace_line: i64) -> Span {
    rb.expr.as_ref()?;
    boxed(shift_lines(&rb.span, i64::from(rb.span.start_line) - brace_line))
}

/// The clean-sheet body's parse: a single element pointer is a function reference, anything else an inline body.
fn clean_sheet_realization(
    body: Vec<ValueSpecification>,
) -> (
    Option<Box<crate::records::PackageableElementPtr>>,
    Option<Box<LambdaFunction>>,
) {
    if body.len() == 1 && matches!(body[0], ValueSpecification::PackageableElementPtr(_)) {
        let Some(ValueSpecification::PackageableElementPtr(ptr)) = body.into_iter().next() else {
            unreachable!("matched above")
        };
        return (Some(ptr), None);
    }
    (
        None,
        Some(Box::new(LambdaFunction {
            parameters: Vec::new(),
            body,
            pos: None,
        })),
    )
}

/// `withSpanStart(op, startTok)`'s arithmetic: the op's span from `start`'s start to its own end.
fn span_from(start: &SourceInfo, op: &PRelOp) -> SourceInfo {
    let end = rel_op_span(op);
    SourceInfo {
        source_id: JStr::default(),
        start_line: start.start_line,
        start_column: start.start_column,
        end_line: end.end_line,
        end_column: end.end_column,
    }
}

/// A mapping element's head, past its `:`.
struct ElementHead<'h> {
    target: &'h [u16],
    target_span: &'h SourceInfo,
    member_start: usize,
    id: Option<JStr>,
    extends_id: Option<JStr>,
    root: bool,
}

impl MappingCursor<'_, '_> {
    /// `parseMapping`: `Mapping qn ( includes* elements* MappingTests? testSuites? )`, each section in its order.
    fn parse_mapping(&mut self) -> R<PMapping> {
        let decl_start = self.pos;
        self.expect(T::MAPPING)?;
        let qn = unquote_path_iae(&self.parse_qualified_name()?)?;
        let (pkg, name) = split_last(&qn);
        self.expect(T::PAREN_OPEN)?;
        let mut assoc_mappings = Vec::new();
        let mut class_mappings = Vec::new();
        let mut enums = Vec::new();
        let mut includes = Vec::new();
        let mut suites = Vec::new();
        let mut legacy_tests = Vec::new();
        let mut body_rank = 0;
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            if self.peek() == T::VALID_STRING && eq(self.text(), "MappingTests") {
                body_rank = self.body_section(body_rank, BODY_LEGACY_TESTS, "MappingTests")?;
                self.advance()?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    legacy_tests.push(Box::new(self.parse_legacy_test(&qn)?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                continue;
            }
            if self.peek() == T::MAPPING_TESTABLE_SUITES {
                body_rank = self.body_section(body_rank, BODY_TEST_SUITES, "testSuites")?;
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    suites.push(Box::new(self.parse_test_suite(&qn)?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                continue;
            }
            let include = self.peek() == T::INCLUDE;
            body_rank = self.body_section(
                body_rank,
                if include { BODY_INCLUDE } else { BODY_ELEMENT },
                if include { "include" } else { "a mapping element" },
            )?;
            self.parse_member(&mut assoc_mappings, &mut class_mappings, &mut enums, &mut includes)?;
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(PMapping {
            pkg: JStr(pkg),
            name: JStr(name),
            association_mappings: assoc_mappings,
            class_mappings,
            enumeration_mappings: enums,
            included_mappings: includes,
            test_suites: suites,
            tests: legacy_tests,
            source_information: self.span(decl_start, self.pos - 1),
        })
    }

    /// `parseMember`: an include (of a mapping, its store substitutions, or a dataspace), or one mapping element:
    /// `*? target [id]? extends [id]? : Kind body`, by its kind.
    fn parse_member(
        &mut self,
        assoc_mappings: &mut Vec<PAssociationMapping>,
        class_mappings: &mut Vec<PClassMapping>,
        enums: &mut Vec<Box<PEnumerationMapping>>,
        includes: &mut Vec<Box<PMappingInclude>>,
    ) -> R<()> {
        if self.peek() == T::INCLUDE {
            let s = self.pos;
            self.advance()?;
            if self.peek() == T::SERVICE_MAPPING && self.peek_at(1) != T::PATH_SEPARATOR {
                self.advance()?;
            } else if self.peek() == T::VALID_STRING && eq(self.text(), "dataspace") {
                self.advance()?;
                let ds = unquote_path_iae(&self.parse_qualified_name()?)?;
                includes.push(Box::new(PMappingInclude {
                    included_mapping: None,
                    included_data_space: Some(JStr(ds)),
                    source_database_path: None,
                    target_database_path: None,
                    substitutions: Vec::new(),
                    source_information: self.span(s, self.pos - 1),
                }));
                return Ok(());
            }
            let inc = unquote_path_iae(&self.parse_qualified_name()?)?;
            let mut src_db = None;
            let mut tgt_db = None;
            let mut subs = Vec::new();
            if self.peek() == T::BRACKET_OPEN {
                self.advance()?;
                while self.peek() != T::BRACKET_CLOSE && !self.at_end() {
                    let src = unquote_path_iae(&self.parse_qualified_name()?)?;
                    self.expect(T::ARROW)?;
                    let tgt = unquote_path_iae(&self.parse_qualified_name()?)?;
                    self.match_tok(T::COMMA)?;
                    subs.push(Box::new(PStoreSubstitution {
                        source_database_path: JStr(src.clone()),
                        target_database_path: JStr(tgt.clone()),
                    }));
                    src_db = Some(JStr(src));
                    tgt_db = Some(JStr(tgt));
                }
                self.expect(T::BRACKET_CLOSE)?;
                if subs.len() > 1 {
                    // the engine records neither path for more than one pair; lite's dialect keeps the pairs
                    src_db = None;
                    tgt_db = None;
                    if self.dialect.refuses_lite_extensions() {
                        subs = Vec::new();
                    }
                }
            }
            includes.push(Box::new(PMappingInclude {
                included_mapping: Some(JStr(inc)),
                included_data_space: None,
                source_database_path: src_db,
                target_database_path: tgt_db,
                substitutions: subs,
                source_information: self.span(s, self.pos - 1),
            }));
            return Ok(());
        }
        let member_start = self.pos;
        let root = self.match_tok(T::STAR)?;
        let target_start = self.pos;
        let target = unquote_path_iae(&self.parse_qualified_name()?)?;
        let target_span = self.span_of(target_start, self.pos - 1);
        let mut id = None;
        if self.peek() == T::BRACKET_OPEN {
            self.advance()?;
            id = Some(JStr(self.parse_set_id()?));
            self.expect(T::BRACKET_CLOSE)?;
        }
        let mut extends_id = None;
        if self.peek() == T::EXTENDS {
            self.advance()?;
            self.expect(T::BRACKET_OPEN)?;
            extends_id = Some(JStr(self.parse_identifier()?));
            self.expect(T::BRACKET_CLOSE)?;
        }
        self.expect(T::COLON)?;
        let body_start = self.pos;
        let mut read_body = |this: &mut Self| {
            this.parse_element_body(
                assoc_mappings,
                class_mappings,
                enums,
                ElementHead {
                    target: &target,
                    target_span: &target_span,
                    member_start,
                    id: id.clone(),
                    extends_id: extends_id.clone(),
                    root,
                },
            )
        };
        // the engine's section parse takes an element's body whole, its walk parses the body and then walks it: inside the
        // body, too, a syntax error comes before what the walk refuses (the user's ruling 16 of 2026-11-09,
        // docs/rust-port/PHASE2.md section 6; PARITY row 63)
        let read = read_body(self);
        if self.dialect.reads_for_syntax() {
            return match (read, self.past_the_element_body(body_start)) {
                (Err(_), Some(past)) => {
                    self.pos = past;
                    Ok(())
                }
                (read, _) => read,
            };
        }
        read.map_err(|e| {
            let level = self.dialect;
            let raised_by_the_walk = e.is_in_flight();
            let answer = e.syntax_first(level, || {
                self.pos = body_start;
                self.dialect = Dialect::LegendEngineSyntax;
                let again = read_body(self);
                self.dialect = level;
                again
            });
            if raised_by_the_walk && answer.is_parse_exception() {
                answer.as_refusal()
            } else {
                answer
            }
        })
    }

    /// The token after a mapping element's `{ ... }`, from the token before its kind; none if the body is left open.
    fn past_the_element_body(&self, from: usize) -> Option<usize> {
        let mut depth = 0i64;
        for t in from..self.tokens.count() {
            match self.tokens.ty(t) {
                T::BRACE_OPEN => depth += 1,
                T::BRACE_CLOSE if depth == 1 => return Some(t + 1),
                T::BRACE_CLOSE => depth -= 1,
                _ => {}
            }
        }
        None
    }

    /// A mapping element past its `:`, by its kind.
    fn parse_element_body(
        &mut self,
        assoc_mappings: &mut Vec<PAssociationMapping>,
        class_mappings: &mut Vec<PClassMapping>,
        enums: &mut Vec<Box<PEnumerationMapping>>,
        head: ElementHead<'_>,
    ) -> R<()> {
        let ElementHead {
            target,
            target_span,
            member_start,
            id,
            extends_id,
            root,
        } = head;
        let kind = self.safe_text();
        if self.peek() == T::ENUMERATION_MAPPING {
            self.advance()?;
            enums.push(Box::new(self.parse_enumeration_mapping(
                target,
                member_start,
                target_span,
            )?));
            return Ok(());
        }
        if self.peek() == T::ASSOCIATION_MAPPING && self.clean_sheet_ahead_at(1)? {
            self.advance()?;
            let m = self.parse_function_association_mapping(target, member_start, target_span)?;
            assoc_mappings.push(PAssociationMapping::PFunctionAssociationMapping(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::RELATIONAL {
            self.advance()?;
            if self.peek_is_association_body() {
                let m = self.parse_rel_association_mapping(target, member_start, target_span, id)?;
                assoc_mappings.push(PAssociationMapping::PRelAssociationMapping(Box::new(m)));
                return Ok(());
            }
            if self.peek_at(1) != T::BRACE_OPEN && self.clean_sheet_ahead()? {
                let m = self.parse_function_class_mapping(
                    target,
                    member_start,
                    target_span,
                    id,
                    extends_id,
                    root,
                    "RELATIONAL",
                )?;
                class_mappings.push(PClassMapping::PClassMappingFunction(Box::new(m)));
                return Ok(());
            }
            let m = self.parse_relational_class_mapping(target, member_start, target_span, id, root, extends_id)?;
            class_mappings.push(PClassMapping::PClassMappingRel(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "XStore") {
            self.advance()?;
            let m = self.parse_xstore_association_mapping(target, member_start, target_span, id)?;
            assoc_mappings.push(PAssociationMapping::PXStoreAssociationMapping(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::PURE_MAPPING {
            self.advance()?;
            if self.clean_sheet_ahead()? {
                let m =
                    self.parse_function_class_mapping(target, member_start, target_span, id, extends_id, root, "PURE")?;
                class_mappings.push(PClassMapping::PClassMappingFunction(Box::new(m)));
                return Ok(());
            }
            let m = self.parse_pure_class_mapping(target, member_start, target_span, id, root, extends_id)?;
            class_mappings.push(PClassMapping::PClassMappingPure(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "AggregationAware") {
            self.advance()?;
            let m = self.parse_aggregation_aware(target, member_start, target_span, id, root, extends_id)?;
            class_mappings.push(PClassMapping::PClassMappingAggregationAware(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "ModelJoin") {
            self.advance()?;
            let m = self.parse_model_join(target, member_start, target_span, id)?;
            assoc_mappings.push(PAssociationMapping::PModelJoinAssociationMapping(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "Relation") {
            self.advance()?;
            let m = self.parse_relation_class_mapping(target, member_start, target_span, id, extends_id, root)?;
            class_mappings.push(PClassMapping::PClassMappingRelation(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "Operation") {
            self.advance()?;
            class_mappings.push(self.parse_operation_class_mapping(
                target,
                member_start,
                target_span,
                id,
                extends_id,
                root,
            )?);
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(&kind, "ServiceStore") {
            if extends_id.is_some() {
                return Err(self.error_str("Service Store Mapping does not support extends"));
            }
            self.advance()?;
            let m = self.parse_service_store_class_mapping(target, member_start, target_span, id, root)?;
            class_mappings.push(PClassMapping::PServiceStoreClassMapping(Box::new(m)));
            return Ok(());
        }
        if self.peek() == T::VALID_STRING && eq(&kind, "MongoDB") {
            self.advance()?;
            let m = self.parse_mongo_db_class_mapping(target, id, root)?;
            class_mappings.push(PClassMapping::PClassMappingMongoDb(Box::new(m)));
            return Ok(());
        }
        if extends_id.is_some() {
            return Err(self.error_str("'extends' on this class-mapping kind is unbuilt"));
        }
        Err(self.error(jcat!("unsupported class mapping type: '", kind, "'")))
    }

    /// `parseServiceStoreClassMapping`: `{ +local: T[m]; ~service [store] G.S (~path ...)? (~request (...))? ... }`.
    fn parse_service_store_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        root: bool,
    ) -> R<PServiceStoreClassMapping> {
        self.expect(T::BRACE_OPEN)?;
        let mut local_props = Vec::new();
        let mut services = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            if self.peek() == T::PLUS {
                let lp_start = self.pos;
                self.advance()?;
                let lp_name = self.parse_identifier()?;
                self.expect(T::COLON)?;
                let lp_type = self.parse_identifier()?;
                let mult = match self.parse_multiplicity()? {
                    Multiplicity::MultiplicityConcrete(c) => c,
                    // the engine's walker fails on the parameter uncaught: its section catch-all answers (PARITY row 28)
                    Multiplicity::MultiplicityParameter(_) => {
                        return Err(self.section_refusal(JStr::from(
                            "a ServiceStore local property's multiplicity must be concrete, not a parameter",
                        )));
                    }
                };
                self.expect(T::SEMI_COLON)?;
                local_props.push(Box::new(PServiceStoreLocalProp {
                    name: JStr(lp_name),
                    r#type: JStr(lp_type),
                    lower_bound: mult.lower_bound,
                    upper_bound: mult.upper_bound,
                    source_information: self.span(lp_start, self.pos - 1),
                }));
                continue;
            }
            let svc_start = self.pos;
            self.expect(T::TILDE)?;
            let kw = self.parse_identifier()?;
            if !eq(&kw, "service") {
                return Err(self.error(jcat!("unknown ServiceStore mapping directive: ~", kw)));
            }
            self.expect(T::BRACKET_OPEN)?;
            let store = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::BRACKET_CLOSE)?;
            let mut segments = Vec::new();
            loop {
                let seg_start = self.pos;
                let seg = self.parse_identifier()?;
                segments.push(Box::new(PServiceSegment {
                    name: JStr(seg),
                    source_information: self.span(seg_start, self.pos - 1),
                }));
                if !self.match_tok(T::DOT)? {
                    break;
                }
            }
            let service = |store: &[u16], segments: Vec<Box<PServiceSegment>>| {
                Box::new(PServicePtr {
                    service_store: JStr(store.to_vec()),
                    segments,
                })
            };
            let mut path_offset = None;
            if self.peek() == T::PAREN_OPEN {
                self.advance()?;
                while self.peek() == T::TILDE && eq(self.peek_text(1), "path") {
                    self.advance()?;
                    self.parse_identifier()?;
                    self.expect(T::DOLLAR)?;
                    let v = self.parse_identifier()?;
                    if !eq(&v, "service") {
                        return Err(self.error_str("~path must start at $service"));
                    }
                    self.expect(T::DOT)?;
                    let r = self.parse_identifier()?;
                    if !eq(&r, "response") {
                        return Err(self.error_str("~path must start at $service.response"));
                    }
                    let mut segs = Vec::new();
                    while self.match_tok(T::DOT)? {
                        segs.push(JStr(self.parse_identifier()?));
                    }
                    path_offset = Some(path_offset_of(segs));
                }
                if self.peek() == T::PAREN_CLOSE {
                    let outer_end0 = self.pos;
                    self.advance()?;
                    services.push(Box::new(PServiceMapping {
                        service: service(&store, segments),
                        path_offset: path_offset.map(Box::new),
                        request: None,
                        source_information: self.span(svc_start, outer_end0),
                    }));
                    continue;
                }
                if self.peek() == T::TILDE && !eq(self.peek_text(1), "request") {
                    services.push(Box::new(self.parse_legacy_service_tail(
                        &store,
                        segments,
                        path_offset,
                        None,
                        svc_start,
                    )?));
                    continue;
                }
                if self.peek() != T::TILDE {
                    services.push(Box::new(self.parse_legacy_service_tail(
                        &store,
                        segments,
                        path_offset,
                        None,
                        svc_start,
                    )?));
                    continue;
                }
                let req_start = self.pos;
                self.expect(T::TILDE)?;
                let rkw = self.parse_identifier()?;
                if !eq(&rkw, "request") {
                    return Err(self.error(jcat!("unknown service-mapping block: ~", rkw)));
                }
                self.expect(T::PAREN_OPEN)?;
                let mut params = None;
                let mut body = None;
                while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                    let key_start = self.pos;
                    let key = self.parse_identifier()?;
                    if eq(&key, "parameters") {
                        self.expect(T::PAREN_OPEN)?;
                        let mut entries = Vec::new();
                        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                            let e_start = self.pos;
                            let pname = if self.peek() == T::QUOTED_STRING {
                                let n = strip_double_quotes(self.text());
                                self.advance()?;
                                n
                            } else {
                                self.parse_identifier()?
                            };
                            self.expect(T::EQUAL)?;
                            let expr_start = self.pos;
                            let expr = self.parse_transform_expr()?;
                            entries.push(Box::new(PParameterBuildInfo {
                                service_parameter: JStr(pname),
                                transform: expr,
                                transform_span: self.span(expr_start, self.pos - 1),
                                source_information: self.span(e_start, self.pos - 1),
                            }));
                            self.match_tok(T::COMMA)?;
                        }
                        let p_end = self.pos;
                        self.expect(T::PAREN_CLOSE)?;
                        params = Some(Box::new(PParametersBuildInfo {
                            entries,
                            source_information: self.span(key_start, p_end),
                        }));
                    } else if eq(&key, "body") {
                        self.expect(T::EQUAL)?;
                        let expr_start = self.pos;
                        let expr = self.parse_transform_expr()?;
                        body = Some(Box::new(PBodyBuildInfo {
                            transform: expr,
                            transform_span: self.span(expr_start, self.pos - 1),
                            source_information: self.span(key_start, self.pos - 1),
                        }));
                    } else {
                        return Err(throw_at(self.tokens, key_start, jcat!("unknown ~request key: ", key)));
                    }
                }
                let req_end = self.pos;
                self.expect(T::PAREN_CLOSE)?;
                let request = PRequestBuildInfo {
                    parameters: params,
                    body,
                    source_information: self.span(req_start, req_end),
                };
                if self.peek() != T::PAREN_CLOSE {
                    services.push(Box::new(self.parse_legacy_service_tail(
                        &store,
                        segments,
                        path_offset,
                        Some(request),
                        svc_start,
                    )?));
                    continue;
                }
                let outer_end = self.pos;
                self.expect(T::PAREN_CLOSE)?;
                services.push(Box::new(PServiceMapping {
                    service: service(&store, segments),
                    path_offset: path_offset.map(Box::new),
                    request: Some(Box::new(request)),
                    source_information: self.span(svc_start, outer_end),
                }));
            } else {
                services.push(Box::new(PServiceMapping {
                    service: service(&store, segments),
                    path_offset: None,
                    request: None,
                    source_information: self.span(svc_start, self.pos - 1),
                }));
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        Ok(PServiceStoreClassMapping {
            class_name: JStr(target.to_vec()),
            class_span: boxed(target_span.clone()),
            id,
            root,
            local_props,
            services,
            source_information: self.span(member_start, self.pos - 1),
            extends_class_mapping_id: None,
        })
    }

    /// `ServiceLegacyMappingParser.parseTail`: the legacy service-mapping tail -- `~paramMapping(p: expr, ...)` and/or
    /// a bare `prop: $service.parameters.p` list -- through the outer `)`; a fresh unspanned parameters envelope
    /// replaces any `~request`'s.
    fn parse_legacy_service_tail(
        &mut self,
        store: &[u16],
        segments: Vec<Box<PServiceSegment>>,
        path_offset: Option<PPathOffset>,
        request: Option<PRequestBuildInfo>,
        svc_start: usize,
    ) -> R<PServiceMapping> {
        let mut legacy = Vec::new();
        if self.peek() == T::TILDE && eq(self.peek_text(1), "paramMapping") {
            self.advance()?;
            self.parse_identifier()?;
            self.expect(T::PAREN_OPEN)?;
            while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                let e_start = self.pos;
                let pname = if self.peek() == T::QUOTED_STRING {
                    let n = strip_double_quotes(self.text());
                    self.advance()?;
                    n
                } else {
                    self.parse_identifier()?
                };
                self.expect(T::COLON)?;
                let expr_start = self.pos;
                let expr = self.parse_transform_expr()?;
                legacy.push(Box::new(PParameterBuildInfo {
                    service_parameter: JStr(pname),
                    transform: expr,
                    transform_span: self.span(expr_start, self.pos - 1),
                    source_information: self.span(e_start, self.pos - 1),
                }));
                self.match_tok(T::COMMA)?;
            }
            self.expect(T::PAREN_CLOSE)?;
        }
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let e_start = self.pos;
            let prop = self.parse_identifier()?;
            self.expect(T::COLON)?;
            self.expect(T::DOLLAR)?;
            if !eq(&self.parse_identifier()?, "service") {
                return Err(self.error_str("elementMapping must read $service.parameters.<name>"));
            }
            self.expect(T::DOT)?;
            if !eq(&self.parse_identifier()?, "parameters") {
                return Err(self.error_str("elementMapping must read $service.parameters.<name>"));
            }
            self.expect(T::DOT)?;
            let pname = self.parse_identifier()?;
            let entry_span = self.span_of(e_start, self.pos - 1);
            // the walker re-parses "$this.<prop>" anchored at the entry: $this five columns, the property after the dot
            let (line, col) = (entry_span.start_line, entry_span.start_column);
            let synthetic = |col: i32, width: i32| SourceInfo {
                source_id: JStr::default(),
                start_line: line,
                start_column: col,
                end_line: line,
                end_column: col + width - 1,
            };
            let var_span = synthetic(col, 5);
            let prop_span = synthetic(col + 6, prop.len() as i32);
            let expr = ValueSpecification::AppliedProperty(Box::new(crate::records::AppliedProperty {
                receiver: ValueSpecification::Variable(Box::new(Variable {
                    name: JStr::from("this"),
                    r#type: None,
                    multiplicity: None,
                    pos: boxed(var_span),
                })),
                property: JStr(prop),
                pos: boxed(prop_span),
                owner_class: None,
            }));
            legacy.push(Box::new(PParameterBuildInfo {
                service_parameter: JStr(pname),
                transform: expr,
                transform_span: boxed(entry_span.clone()),
                source_information: boxed(entry_span),
            }));
            self.match_tok(T::COMMA)?;
        }
        let outer_end = self.pos;
        self.expect(T::PAREN_CLOSE)?;
        let params = Box::new(PParametersBuildInfo {
            entries: legacy,
            source_information: None,
        });
        let merged = match request {
            None => PRequestBuildInfo {
                parameters: Some(params),
                body: None,
                source_information: None,
            },
            Some(r) => PRequestBuildInfo {
                parameters: Some(params),
                body: r.body,
                source_information: r.source_information,
            },
        };
        Ok(PServiceMapping {
            service: Box::new(PServicePtr {
                service_store: JStr(store.to_vec()),
                segments,
            }),
            path_offset: path_offset.map(Box::new),
            request: Some(Box::new(merged)),
            source_information: self.span(svc_start, outer_end),
        })
    }

    /// `parseTransformExpr`: one expression up to the next top-level `,` or `)`, parsed on a slice of the shared stream.
    fn parse_transform_expr(&mut self) -> R<ValueSpecification> {
        let bs = self.pos;
        let mut d = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if is_open(t) {
                d += 1;
            } else if is_close(t) {
                if d == 0 {
                    break;
                }
                d -= 1;
            } else if t == T::COMMA && d == 0 {
                break;
            }
            self.advance()?;
        }
        parse_tokens(&self.tokens.slice(bs, self.pos), self.dialect)
    }

    /// `parseMongoDbClassMapping`: `{ ~mainCollection [db] Coll ~filter [db] name ~binding path }` -- no spans, the
    /// filter parsed and dropped.
    fn parse_mongo_db_class_mapping(
        &mut self,
        target: &[u16],
        id: Option<JStr>,
        root: bool,
    ) -> R<PClassMappingMongoDb> {
        self.expect(T::BRACE_OPEN)?;
        let mut store = None;
        let mut coll = None;
        let mut binding = None;
        while !self.at_end() && (self.peek() == T::TILDE || self.peek() == T::FILTER_CMD) {
            let kw = if self.peek() == T::FILTER_CMD {
                self.advance()?;
                u("filter")
            } else {
                self.advance()?;
                self.parse_identifier()?
            };
            if eq(&kw, "mainCollection") {
                self.expect(T::BRACKET_OPEN)?;
                store = Some(JStr(unquote_path_iae(&self.parse_qualified_name()?)?));
                self.expect(T::BRACKET_CLOSE)?;
                coll = Some(JStr(self.mongo_name()?));
            } else if eq(&kw, "filter") {
                self.expect(T::BRACKET_OPEN)?;
                unquote_path_iae(&self.parse_qualified_name()?)?;
                self.expect(T::BRACKET_CLOSE)?;
                self.mongo_name()?;
            } else if eq(&kw, "binding") {
                binding = Some(JStr(unquote_path_iae(&self.parse_qualified_name()?)?));
            } else {
                return Err(self.error(jcat!("unknown MongoDB mapping directive: ~", kw)));
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        if store.is_none() || coll.is_none() {
            return Err(self.error_str("MongoDB mapping needs ~mainCollection"));
        }
        Ok(PClassMappingMongoDb {
            class_name: JStr(target.to_vec()),
            id,
            root,
            store_path: store,
            main_collection_name: coll,
            binding_path: binding,
            extends_class_mapping_id: None,
        })
    }

    /// `mongoName`: a collection or filter name -- an identifier, or a `~`-spelled keyword kept with its tilde.
    fn mongo_name(&mut self) -> R<Vec<u16>> {
        if self.peek() == T::TILDE {
            self.advance()?;
            let mut n = u("~");
            n.extend(self.parse_identifier()?);
            return Ok(n);
        }
        if !self.at_end() && self.text().first() == Some(&u16::from(b'~')) {
            let t = self.text().to_vec();
            self.advance()?;
            return Ok(t);
        }
        self.parse_identifier()
    }

    /// `cleanSheetAheadAt(ahead)`: [`Self::clean_sheet_ahead`] with the kind keyword `ahead` tokens back.
    fn clean_sheet_ahead_at(&mut self, ahead: usize) -> R<bool> {
        let save = self.pos;
        for _ in 0..ahead {
            self.advance()?;
        }
        let clean = self.clean_sheet_ahead();
        self.pos = save;
        clean
    }

    /// `cleanSheetAhead`: the body at the brace is the clean-sheet function form (never at the engine's dialect).
    fn clean_sheet_ahead(&mut self) -> R<bool> {
        if self.dialect.refuses_lite_extensions() {
            return Ok(false);
        }
        if self.peek() != T::BRACE_OPEN {
            return Ok(false);
        }
        let save = self.pos;
        self.advance()?;
        let clean = self.is_clean_sheet_body();
        self.pos = save;
        Ok(clean)
    }

    /// The braced body of a clean-sheet binding, read as a code block: its statements and the closing brace's token.
    fn clean_sheet_body(&mut self) -> R<(Vec<ValueSpecification>, usize)> {
        self.expect(T::BRACE_OPEN)?;
        let body_start = self.pos;
        let mut depth = 1i64;
        while !self.at_end() && depth > 0 {
            let t = self.peek();
            if t == T::BRACE_OPEN {
                depth += 1;
            } else if t == T::BRACE_CLOSE {
                depth -= 1;
            }
            if depth > 0 {
                self.advance()?;
            }
        }
        let body = parse_code_block_tokens(&self.tokens.slice(body_start, self.pos), JStr::default(), self.dialect)?;
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok((body, close))
    }

    /// `parseFunctionAssociationMapping`: `assoc: AssociationMapping { fn-or-body }`, the clean-sheet binding.
    fn parse_function_association_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
    ) -> R<PFunctionAssociationMapping> {
        let (body, close) = self.clean_sheet_body()?;
        if body.is_empty() {
            return Err(self.error(jcat!(
                "clean-sheet association binding for '",
                target,
                "' has an empty body"
            )));
        }
        let (function, body_lambda) = clean_sheet_realization(body);
        Ok(PFunctionAssociationMapping {
            association: pointer("ASSOCIATION", target, target_span),
            function,
            body_lambda,
            source_information: self.span(member_start, close),
        })
    }

    /// `parseFunctionClassMapping`: `cls: Relational|Pure { fn-or-body }`, the clean-sheet function form.
    #[allow(clippy::too_many_arguments)]
    fn parse_function_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        extends_id: Option<JStr>,
        root: bool,
        kind: &str,
    ) -> R<PClassMappingFunction> {
        let (body, close) = self.clean_sheet_body()?;
        if body.is_empty() {
            return Err(self.error(jcat!(
                "clean-sheet mapping binding for '",
                target,
                "' has an empty body"
            )));
        }
        let (function, body_lambda) = clean_sheet_realization(body);
        Ok(PClassMappingFunction {
            class_name: JStr(target.to_vec()),
            class_source_information: boxed(target_span.clone()),
            id,
            extends_class_mapping_id: extends_id,
            root,
            kind: JStr::from(kind),
            function,
            body_lambda,
            source_information: self.span(member_start, close),
        })
    }

    /// `bindingTransformerId`: `Binding qualifiedName :` before a property mapping's operation, or none.
    fn binding_transformer_id(&mut self) -> R<Option<JStr>> {
        if self.peek() == T::VALID_STRING && eq(self.text(), "Binding") && is_identifier_token(self.peek_at(1)) {
            self.advance()?;
            let fqn = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::COLON)?;
            return Ok(Some(JStr(fqn)));
        }
        Ok(None)
    }

    /// `enumerationMappingId`: `EnumerationMapping <id>:` before a property mapping's right-hand side, or none; the id
    /// is required once the keyword is written.
    fn enumeration_mapping_id(&mut self) -> R<Option<JStr>> {
        if self.peek() != T::ENUMERATION_MAPPING {
            return Ok(None);
        }
        self.advance()?;
        if self.peek() == T::COLON {
            return Err(self.error_str(
                "EnumerationMapping on a property mapping requires an enumeration mapping id (prop: \
                 EnumerationMapping <id>: <expr>)",
            ));
        }
        let enum_id = self.parse_mapping_word()?;
        self.expect(T::COLON)?;
        Ok(Some(JStr(enum_id)))
    }

    /// `relationBinding(stop)`: a Relation binding's right-hand side up to `stop` (or a `,`) at depth 0 -- a bare or
    /// quoted column name, else a row expression.
    fn relation_binding(&mut self, stop: TokenType) -> R<RelationBinding> {
        let expr_start = self.pos;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if depth == 0 && (t == T::COMMA || t == stop) {
                break;
            }
            if is_open(t) {
                depth += 1;
            } else if is_close(t) {
                depth -= 1;
                if depth < 0 {
                    break;
                }
            }
            self.advance()?;
        }
        let rhs = parse_tokens(&self.tokens.slice(expr_start, self.pos), self.dialect)?;
        let span = self.span_of(expr_start, self.pos - 1);
        match rhs {
            ValueSpecification::CString(cs) => Ok(RelationBinding {
                column: Some(cs.value),
                expr: None,
                span,
            }),
            ValueSpecification::PackageableElementPtr(ptr)
                if index_of_units(&ptr.full_path.0, &u("::"), 0).is_none() =>
            {
                Ok(RelationBinding {
                    column: Some(ptr.full_path),
                    expr: None,
                    span,
                })
            }
            rhs => Ok(RelationBinding {
                column: None,
                expr: Some(rhs),
                span,
            }),
        }
    }

    /// `bodySection(seen, rank, what)`: one mapping-body section admitted, or its swap or repeat refused.
    fn body_section(&self, seen: i32, rank: i32, what: &str) -> R<i32> {
        if rank < seen {
            return Err(self.error(jcat!(
                what,
                " is out of order: a mapping body holds its includes, then its elements, then MappingTests, then \
                 testSuites"
            )));
        }
        if rank == seen && rank >= BODY_LEGACY_TESTS {
            return Err(self.error(jcat!("duplicate ", what, ": a mapping declares it at most once")));
        }
        Ok(rank)
    }

    /// `clause(seen, rank, name)`: one `~clause` admitted, or its repeat or swap refused.
    fn clause(&self, seen: &mut i32, rank: i32, name: &str) -> R<()> {
        if rank <= *seen {
            return Err(self.error(jcat!(
                name,
                " is out of order: a relational class mapping's directives appear at most once each, in the order \
                 ~filter ~distinct ~groupBy ~primaryKey ~mainTable, and all of them before the first property mapping"
            )));
        }
        *seen = rank;
        Ok(())
    }

    /// `parseRelationalClassMapping`: `{ ~filter ~distinct ~groupBy(...) ~primaryKey(...) ~mainTable [db]T props }`.
    pub(crate) fn parse_relational_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        root: bool,
        extends_id: Option<JStr>,
    ) -> R<PClassMappingRel> {
        self.expect(T::BRACE_OPEN)?;
        let mut filter = None;
        let mut main_table = None;
        let mut primary_key = Vec::new();
        let mut group_by = Vec::new();
        let mut distinct = false;
        let mut props = Vec::new();
        let mut clause_rank = 0;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            if self.peek() == T::MAIN_TABLE_CMD || (self.peek() == T::TILDE && eq(self.peek_text(1), "mainTable")) {
                self.clause(&mut clause_rank, RANK_MAIN_TABLE, "~mainTable")?;
                if self.peek() == T::TILDE {
                    self.advance()?;
                }
                self.advance()?;
                self.expect(T::BRACKET_OPEN)?;
                let db = unquote_path_iae(&self.parse_qualified_name()?)?;
                self.expect(T::BRACKET_CLOSE)?;
                let t_s = self.pos;
                let mut schema = u("default");
                let mut tbl = self.parse_maybe_quoted_identifier()?;
                if self.peek() == T::DOT {
                    self.advance()?;
                    schema = tbl;
                    tbl = self.parse_maybe_quoted_identifier()?;
                }
                main_table = Some(Box::new(PTablePtr {
                    database: Some(JStr(db.clone())),
                    main_table_db: Some(JStr(db)),
                    schema: JStr(schema),
                    table: JStr(tbl),
                    source_information: self.span(t_s, self.pos - 1),
                    written_type: None,
                }));
                continue;
            }
            if self.peek() == T::PRIMARY_KEY_CMD {
                self.clause(&mut clause_rank, RANK_PRIMARY_KEY, "~primaryKey")?;
                self.advance()?;
                self.expect(T::PAREN_OPEN)?;
                while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                    primary_key.push(self.parse_embedded_operation()?);
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::PAREN_CLOSE)?;
                continue;
            }
            if self.peek() == T::DISTINCT_CMD {
                self.clause(&mut clause_rank, RANK_DISTINCT, "~distinct")?;
                self.advance()?;
                distinct = true;
                continue;
            }
            if self.peek() == T::GROUP_BY_CMD {
                self.clause(&mut clause_rank, RANK_GROUP_BY, "~groupBy")?;
                self.advance()?;
                self.expect(T::PAREN_OPEN)?;
                while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                    group_by.push(self.parse_embedded_operation()?);
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::PAREN_CLOSE)?;
                continue;
            }
            if self.peek() == T::FILTER_CMD {
                self.clause(&mut clause_rank, RANK_FILTER, "~filter")?;
                filter = Some(Box::new(self.parse_filter_mapping()?));
                continue;
            }
            if self.peek() == T::TILDE {
                return Err(self.error(jcat!("class-mapping directive '", self.safe_text(), "' is unbuilt")));
            }
            clause_rank = RANK_PROPERTIES;
            if self.peek() == T::VALID_STRING && eq(self.text(), "scope") && self.peek_at(1) == T::PAREN_OPEN {
                self.parse_scope_block(&mut props, Some(target), id.as_ref())?;
                self.match_tok(T::COMMA)?;
                continue;
            }
            self.parse_property_line(&mut props, Some(target), id.as_ref(), None, None)?;
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PClassMappingRel {
            class_name: JStr(target.to_vec()),
            class_source_information: boxed(target_span.clone()),
            id,
            root,
            distinct,
            extends_class_mapping_id: extends_id,
            filter,
            group_by,
            main_table,
            primary_key,
            property_mappings: props,
            source_information: self.span(member_start, close),
        })
    }

    /// `parsePureClassMapping`: `{ ~src my::S ~filter <expr> prop: <expr>, +local: T[m]: <expr>, ... }`, each
    /// transform ONE expression over a slice of the shared stream.
    pub(crate) fn parse_pure_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        root: bool,
        extends_id: Option<JStr>,
    ) -> R<PClassMappingPure> {
        self.expect(T::BRACE_OPEN)?;
        let mut src_class = None;
        let mut src_span = None;
        let mut filter_body = None;
        let mut props = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            if self.peek() == T::SRC_CMD {
                if src_class.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        member_start,
                        JStr::from("Field '~src' should be specified only once"),
                    ));
                }
                self.advance()?;
                let s_s = self.pos;
                src_class = Some(JStr(unquote_path_iae(&self.parse_qualified_name()?)?));
                src_span = self.span(s_s, self.pos - 1);
                continue;
            }
            if self.peek() == T::FILTER_CMD {
                if filter_body.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        member_start,
                        JStr::from("Field '~filter' should be specified only once"),
                    ));
                }
                self.advance()?;
                let f_start = self.pos;
                let mut depth = 0i64;
                while !self.at_end() {
                    let t = self.peek();
                    if depth == 0 {
                        if matches!(t, T::BRACE_CLOSE | T::SRC_CMD | T::FILTER_CMD | T::PLUS) {
                            break;
                        }
                        if (t == T::VALID_STRING || t == T::STRING)
                            && self.pos + 1 < self.tokens.count()
                            && self.tokens.ty(self.pos + 1) == T::COLON
                        {
                            break;
                        }
                    }
                    if is_open(t) {
                        depth += 1;
                    } else if is_close(t) {
                        depth -= 1;
                    }
                    self.advance()?;
                }
                filter_body = Some(parse_code_block_tokens(
                    &self.tokens.slice(f_start, self.pos),
                    JStr::default(),
                    self.dialect,
                )?);
                continue;
            }
            if self.peek() == T::TILDE {
                return Err(self.error(jcat!(
                    "pure class-mapping directive '",
                    self.safe_text(),
                    "' is unbuilt"
                )));
            }
            let p_s = self.pos;
            let local = self.match_tok(T::PLUS)?;
            let ident_tok = self.pos;
            let prop = self.parse_identifier()?;
            let prop_span = self.span_of(ident_tok, self.pos - 1);
            let src_id = id.clone().unwrap_or_default();
            let mut tgt_id = None;
            if self.peek() == T::BRACKET_OPEN {
                self.advance()?;
                // the walker reads the first bracketed id as the target and drops a second
                tgt_id = Some(JStr(self.parse_set_id()?));
                if self.peek() == T::COMMA {
                    self.advance()?;
                    self.parse_set_id()?;
                }
                self.expect(T::BRACKET_CLOSE)?;
            }
            let explode = self.match_tok(T::STAR)?;
            let mut local_prop = None;
            self.expect(T::COLON)?;
            let enum_id = self.enumeration_mapping_id()?;
            if local {
                let ty = unquote_path_iae(&self.parse_qualified_name()?)?;
                let (lower, upper) = self.parse_long_mult_bounds()?;
                self.expect(T::BRACKET_CLOSE)?;
                local_prop = Some(Box::new(PLocalProp {
                    r#type: JStr(ty),
                    lower_bound: lower,
                    upper_bound: upper,
                    source_information: boxed(prop_span.clone()),
                }));
                self.expect(T::COLON)?;
            }
            let expr_start = self.pos;
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if depth == 0 && (t == T::COMMA || t == T::BRACE_CLOSE) {
                    break;
                }
                if depth == 0 && t == T::SEMI_COLON {
                    return Err(self.error_str(
                        "a property mapping binds ONE expression and is separated by ',' \u{2014} ';' is not a mapping \
                         separator",
                    ));
                }
                if is_open(t) {
                    depth += 1;
                } else if is_close(t) {
                    depth -= 1;
                }
                self.advance()?;
            }
            let body =
                parse_code_block_tokens(&self.tokens.slice(expr_start, self.pos), JStr::default(), self.dialect)?;
            if body.is_empty() {
                return Err(self.error_str("a property mapping has an empty body"));
            }
            if body.len() != 1 {
                return Err(self.error(jcat!("a property mapping binds ONE expression; got ", body.len())));
            }
            props.push(Box::new(PPurePropertyMapping {
                owner_class: if local { None } else { Some(JStr(target.to_vec())) },
                property: JStr(prop),
                property_source_information: boxed(prop_span),
                enum_mapping_id: enum_id,
                explode_property: explode,
                local_mapping_property: local_prop,
                transform: body,
                source: Some(src_id),
                target: tgt_id,
                source_information: self.span(p_s, self.pos - 1),
            }));
            self.entry_comma(&[T::BRACE_CLOSE])?;
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        if src_class.is_none() && filter_body.is_some() {
            return Err(
                self.error_str("a ~filter needs a ~src: the filter reads $src, so the mapping must declare its type")
            );
        }
        Ok(PClassMappingPure {
            class_name: JStr(target.to_vec()),
            class_source_information: boxed(target_span.clone()),
            extends_class_mapping_id: extends_id,
            id,
            root,
            src_class,
            source_class_source_information: src_span,
            filter: filter_body,
            property_mappings: props,
            source_information: self.span(member_start, close),
        })
    }

    /// `peekIsAssociationBody`: the `Relational` body opens with `AssociationMapping`.
    fn peek_is_association_body(&self) -> bool {
        self.peek() == T::BRACE_OPEN
            && self.tokens.count() > self.pos + 1
            && self.tokens.ty(self.pos + 1) == T::ASSOCIATION_MAPPING
    }

    /// `parseRelAssociationMapping`: `{ AssociationMapping ( side[src, tgt]?: [db]@J, ... ) }` -- the stores the join
    /// pointers' databases in order, until the first property that is not a navigation.
    fn parse_rel_association_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
    ) -> R<PRelAssociationMapping> {
        self.expect(T::BRACE_OPEN)?;
        self.expect(T::ASSOCIATION_MAPPING)?;
        self.expect(T::PAREN_OPEN)?;
        let mut props = Vec::new();
        let mut stores: Vec<JStr> = Vec::new();
        let mut stores_closed = false;
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let p_s = self.pos;
            let prop = self.parse_identifier()?;
            let prop_span = self.span(p_s, self.pos - 1);
            let mut src_id = None;
            let mut tgt_id = None;
            if self.peek() == T::BRACKET_OPEN {
                self.advance()?;
                let first = self.parse_set_id()?;
                if self.peek() == T::COMMA {
                    self.advance()?;
                    src_id = Some(JStr(first));
                    tgt_id = Some(JStr(self.parse_set_id()?));
                } else {
                    tgt_id = Some(JStr(first));
                }
                self.expect(T::BRACKET_CLOSE)?;
            }
            self.expect(T::COLON)?;
            let o_s = self.pos;
            let op = self.parse_embedded_operation()?;
            if let PRelOp::PElemtWithJoins(ej) = &op {
                if !stores_closed {
                    for jp in &ej.joins {
                        if let Some(jdb) = &jp.db
                            && !stores.contains(jdb)
                        {
                            stores.push(jdb.clone());
                        }
                    }
                }
            } else {
                stores_closed = true;
            }
            props.push(Box::new(PRelAssocPropertyMapping {
                property: JStr(prop),
                property_source_information: prop_span,
                relational_operation: op,
                source: src_id,
                target: tgt_id,
                source_information: self.span(o_s - 1, self.pos - 1),
                owner_class: None,
            }));
            self.match_tok(T::COMMA)?;
        }
        self.expect(T::PAREN_CLOSE)?;
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PRelAssociationMapping {
            association: pointer("ASSOCIATION", target, target_span),
            id,
            property_mappings: props,
            stores,
            source_information: self.span(member_start, close),
        })
    }

    /// `parseXStoreAssociationMapping`: `{ side[src, tgt]: <cross expression>, ... }` -- a missing comma refused at
    /// the platform-refusing dialects, else the entry ends there; after an entry, a missing comma ends the list and
    /// discards the rest (legend-pure's reading).
    fn parse_xstore_association_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
    ) -> R<PXStoreAssociationMapping> {
        self.expect(T::BRACE_OPEN)?;
        let mut props = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let p_s = self.pos;
            let prop = self.parse_identifier()?;
            let prop_span = self.span(p_s, self.pos - 1);
            let mut src_id = Vec::new();
            let mut tgt_id = Vec::new();
            if self.peek() == T::BRACKET_OPEN {
                self.advance()?;
                let first = self.parse_set_id()?;
                if self.peek() != T::COMMA {
                    return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
                }
                self.advance()?;
                src_id = first;
                tgt_id = self.parse_set_id()?;
                self.expect(T::BRACKET_CLOSE)?;
            }
            self.expect(T::COLON)?;
            let expr_start = self.pos;
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if depth == 0 && (t == T::COMMA || t == T::BRACE_CLOSE) {
                    break;
                }
                if depth == 0
                    && self.pos > expr_start
                    && is_identifier_token(t)
                    && (self.peek_at(1) == T::BRACKET_OPEN || self.peek_at(1) == T::COLON)
                {
                    if self.dialect.refuses_platform_dialect() {
                        return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
                    }
                    break;
                }
                if is_open(t) {
                    depth += 1;
                } else if is_close(t) {
                    depth -= 1;
                }
                self.advance()?;
            }
            let body =
                parse_code_block_tokens(&self.tokens.slice(expr_start, self.pos), JStr::default(), self.dialect)?;
            props.push(Box::new(PXStorePropertyMapping {
                owner_class: JStr(target.to_vec()),
                property: JStr(prop),
                property_source_information: prop_span,
                cross_expression: body,
                source: JStr(src_id),
                target: JStr(tgt_id),
                source_information: self.span(p_s, self.pos - 1),
            }));
            if !self.match_tok(T::COMMA)? && self.peek() != T::BRACE_CLOSE {
                while !self.at_end() && self.peek() != T::BRACE_CLOSE {
                    self.advance()?;
                }
                break;
            }
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PXStoreAssociationMapping {
            association: pointer("ASSOCIATION", target, target_span),
            id,
            property_mappings: props,
            source_information: self.span(member_start, close),
        })
    }

    /// `parseOperationClassMapping`: `{ fqn(p1, p2) }`, the discriminator by the function's exact path; a merge's
    /// `fqn([p1, p2], <validation>)` its validation re-parsed from its tokens' texts joined, anchored at the path.
    fn parse_operation_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        extends_id: Option<JStr>,
        root: bool,
    ) -> R<PClassMapping> {
        self.expect(T::BRACE_OPEN)?;
        let fqn_tok = self.pos;
        let fqn = unquote_path_iae(&self.parse_qualified_name()?)?;
        let operation = operation_discriminator(&fqn).map(JStr::from);
        self.expect(T::PAREN_OPEN)?;
        let mut params = Vec::new();
        if self.peek() == T::BRACKET_OPEN {
            self.advance()?;
            while self.peek() != T::BRACKET_CLOSE && !self.at_end() {
                params.push(JStr(self.parse_set_id()?));
                self.match_tok(T::COMMA)?;
            }
            self.expect(T::BRACKET_CLOSE)?;
            self.expect(T::COMMA)?;
            let l_s = self.pos;
            let mut depth = 0i64;
            while !self.at_end() {
                let t = self.peek();
                if is_open(t) {
                    depth += 1;
                } else if is_close(t) {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                self.advance()?;
            }
            let l_end = self.pos as isize - 1;
            if l_end < l_s as isize || self.at_end() || self.peek() != T::PAREN_CLOSE {
                return Err(self.error_str("merge validation: expected an expression and then ')'"));
            }
            let mut padded: Vec<u16> = Vec::new();
            for _ in 1..self.tokens.start_line(fqn_tok) {
                padded.push(u16::from(b'\n'));
            }
            for _ in 1..self.tokens.start_column(fqn_tok) {
                padded.push(u16::from(b' '));
            }
            for i in l_s..=(l_end as usize) {
                padded.extend_from_slice(self.tokens.text(i));
            }
            let mut body = parse_code_block(&padded, self.dialect)?;
            if body.len() != 1 {
                return Err(self.error_str("merge validation must be ONE lambda"));
            }
            self.expect(T::PAREN_CLOSE)?;
            self.match_tok(T::SEMI_COLON)?;
            let close = self.pos;
            self.expect(T::BRACE_CLOSE)?;
            return Ok(PClassMapping::PClassMappingMergeOperation(Box::new(
                PClassMappingMergeOperation {
                    class_name: JStr(target.to_vec()),
                    class_source_information: boxed(target_span.clone()),
                    id,
                    root,
                    parameters: params,
                    validation_lambda: body.remove(0),
                    source_information: self.span(member_start, close),
                    extends_class_mapping_id: None,
                },
            )));
        }
        while self.peek() != T::PAREN_CLOSE && !self.at_end() {
            params.push(JStr(self.parse_set_id()?));
            self.match_tok(T::COMMA)?;
        }
        self.expect(T::PAREN_CLOSE)?;
        self.match_tok(T::SEMI_COLON)?;
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        // the engine's walker drops an operation's extends; lite's dialect keeps it (S89, rule A)
        Ok(PClassMapping::PClassMappingOperation(Box::new(
            PClassMappingOperation {
                class_name: JStr(target.to_vec()),
                class_source_information: boxed(target_span.clone()),
                id,
                extends_class_mapping_id: if self.dialect.refuses_lite_extensions() {
                    None
                } else {
                    extends_id
                },
                root,
                operation,
                parameters: params,
                source_information: self.span(member_start, close),
            },
        )))
    }

    /// `parseSetId`: a set-implementation id, a number or boolean admitted.
    fn parse_set_id(&mut self) -> R<Vec<u16>> {
        self.parse_mapping_word()
    }

    /// `parseMappingWord`: the engine's `word: identifier | BOOLEAN | INTEGER`.
    fn parse_mapping_word(&mut self) -> R<Vec<u16>> {
        let t = self.peek();
        if matches!(t, T::INTEGER | T::BOOLEAN | T::TRUE | T::FALSE) {
            let w = self.text().to_vec();
            self.advance()?;
            return Ok(w);
        }
        self.parse_identifier()
    }

    /// `parseFilterMapping`: `~filter [db] NAME` or `~filter [db] (TYPE)? @J (> (TYPE)? [db]? @K)* | [db2] NAME`, its
    /// span the `~filter` through the name.
    fn parse_filter_mapping(&mut self) -> R<PFilterMapping> {
        let f_s = self.pos;
        self.advance()?;
        if self.peek() != T::BRACKET_OPEN {
            return Err(self.error_str(
                "a class mapping's ~filter requires a [DB] pointer (~filter [DB] FilterName); only a View's ~filter \
                 may omit it",
            ));
        }
        self.expect(T::BRACKET_OPEN)?;
        let db = unquote_path_iae(&self.parse_qualified_name()?)?;
        self.expect(T::BRACKET_CLOSE)?;
        let mut joins = Vec::new();
        let mut fdb = db.clone();
        let mut first_type = None;
        if self.peek() == T::PAREN_OPEN {
            self.advance()?;
            let t = self.parse_identifier()?;
            self.validate_join_type(&t)?;
            first_type = Some(t);
            self.expect(T::PAREN_CLOSE)?;
        }
        let name = if self.peek() == T::AT {
            let mut cur_db = db;
            let mut pending_type = first_type;
            while self.peek() == T::AT {
                let j_s = self.pos;
                self.advance()?;
                let jn = self.parse_identifier()?;
                joins.push(Box::new(PJoinPtr {
                    db: Some(JStr(cur_db.clone())),
                    join_type: pending_type.take().map(JStr),
                    name: JStr(jn),
                    source_information: self.span(j_s, self.pos - 1),
                }));
                if self.peek() != T::GREATER_THAN {
                    break;
                }
                self.advance()?;
                if self.peek() == T::PAREN_OPEN {
                    self.advance()?;
                    let t = self.parse_identifier()?;
                    self.validate_join_type(&t)?;
                    pending_type = Some(t);
                    self.expect(T::PAREN_CLOSE)?;
                }
                if self.peek() == T::BRACKET_OPEN {
                    self.advance()?;
                    cur_db = unquote_path_iae(&self.parse_qualified_name()?)?;
                    self.expect(T::BRACKET_CLOSE)?;
                }
            }
            self.expect(T::PIPE)?;
            self.expect(T::BRACKET_OPEN)?;
            fdb = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::BRACKET_CLOSE)?;
            self.parse_identifier()?
        } else {
            self.parse_identifier()?
        };
        Ok(PFilterMapping {
            db: JStr(fdb),
            name: JStr(name),
            joins,
            source_information: self.span(f_s, self.pos - 1),
        })
    }

    /// `parsePropertyLine`: one `prop[src, tgt]?: (EnumerationMapping em:)? (Binding b:)? <op>` -- or `+local: T[m]:
    /// <op>`, an embedded `prop ( lines )` (and its `Otherwise ( [tgt]: <op> )`), an inline `prop () Inline[set]`;
    /// one bracketed id is the target, the source staying the enclosing id.
    fn parse_property_line(
        &mut self,
        props: &mut Vec<PPropertyMapping>,
        target: Option<&[u16]>,
        id: Option<&JStr>,
        scope_db: Option<&[u16]>,
        scope: Option<&ScopeCtx>,
    ) -> R<()> {
        let owner = || target.map(|t| JStr(t.to_vec()));
        let mut p_s = self.pos;
        let mut local_prop = None;
        let local = self.match_tok(T::PLUS)?;
        if local {
            p_s = self.pos;
        }
        let prop = self.parse_identifier()?;
        let prop_span = self.span_of(p_s, self.pos - 1);
        let mut src_id = id.cloned();
        let mut tgt_id = None;
        let mut emb_id = None;
        if self.peek() == T::BRACKET_OPEN {
            self.advance()?;
            let first = self.parse_set_id()?;
            if self.peek() == T::COMMA {
                self.advance()?;
                src_id = Some(JStr(first));
                tgt_id = Some(JStr(self.parse_set_id()?));
            } else {
                tgt_id = Some(JStr(first.clone()));
                emb_id = Some(JStr(first));
            }
            self.expect(T::BRACKET_CLOSE)?;
        }
        if self.peek() == T::PAREN_OPEN {
            let paren_tok = self.pos;
            self.advance()?;
            if self.peek() == T::PAREN_CLOSE && eq(self.peek_text(1), "Inline") {
                self.advance()?;
                self.advance()?;
                self.expect(T::BRACKET_OPEN)?;
                let set_id = self.parse_identifier()?;
                let close = self.pos;
                self.expect(T::BRACKET_CLOSE)?;
                props.push(PPropertyMapping::PInlineEmbeddedPropertyMapping(Box::new(
                    PInlineEmbeddedPropertyMapping {
                        owner_class: owner(),
                        property: JStr(prop),
                        property_source_information: boxed(prop_span),
                        id: emb_id,
                        set_implementation_id: JStr(set_id),
                        source_information: self.span(paren_tok, close),
                    },
                )));
                return self.entry_comma(&[T::BRACE_CLOSE, T::PAREN_CLOSE]);
            }
            let mut inner = Vec::new();
            let mut pk = Vec::new();
            while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                if self.peek() == T::PRIMARY_KEY_CMD {
                    self.advance()?;
                    self.expect(T::PAREN_OPEN)?;
                    while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                        pk.push(self.parse_op_in_ctx(scope_db, scope)?);
                        self.match_tok(T::COMMA)?;
                    }
                    self.expect(T::PAREN_CLOSE)?;
                    continue;
                }
                self.parse_property_line(&mut inner, None, id, scope_db, scope)?;
            }
            let close = self.pos;
            self.expect(T::PAREN_CLOSE)?;
            if self.peek() == T::OTHERWISE {
                let otherwise_tok = self.pos;
                self.advance()?;
                self.expect(T::PAREN_OPEN)?;
                let o_s = self.pos;
                self.expect(T::BRACKET_OPEN)?;
                let tgt = self.parse_set_id()?;
                self.expect(T::BRACKET_CLOSE)?;
                self.expect(T::COLON)?;
                if self.dialect.refuses_platform_dialect() {
                    // the engine's otherwise target is a join only; a plain column pointer is the platform's
                    let ts = self.tokens;
                    let mut look = self.pos;
                    let has_db = ty_or_slack(ts, look) == T::BRACKET_OPEN;
                    if has_db {
                        while look < ts.count() && ts.ty(look) != T::BRACKET_CLOSE {
                            look += 1;
                        }
                        look += 1;
                    }
                    if look < ts.count() && ts.ty(look) != T::AT && ts.ty(look) != T::PAREN_OPEN {
                        self.pos = look;
                        return Err(self.error(jcat!(
                            "Unexpected token '",
                            self.safe_text(),
                            "'. Valid alternatives: ",
                            if has_db { "['(', '@']" } else { "['[', '(', '@']" }
                        )));
                    }
                }
                let op = self.parse_op_in_ctx(scope_db, scope)?;
                let op = self.with_span_start(op, o_s);
                let o_close = self.pos;
                self.expect(T::PAREN_CLOSE)?;
                props.push(PPropertyMapping::POtherwiseEmbeddedPropertyMapping(Box::new(
                    POtherwiseEmbeddedPropertyMapping {
                        owner_class: owner(),
                        property: JStr(prop),
                        property_source_information: boxed(prop_span),
                        id: emb_id,
                        primary_key: pk,
                        property_mappings: inner,
                        otherwise_op: op,
                        otherwise_target: JStr(tgt),
                        class_mapping_source_information: self.span(paren_tok, o_close),
                        source_information: self.span(otherwise_tok, o_close),
                        embedded_class: None,
                    },
                )));
                return self.entry_comma(&[T::BRACE_CLOSE, T::PAREN_CLOSE]);
            }
            props.push(PPropertyMapping::PEmbeddedPropertyMapping(Box::new(
                PEmbeddedPropertyMapping {
                    owner_class: owner(),
                    property: JStr(prop),
                    property_source_information: boxed(prop_span),
                    id: emb_id,
                    primary_key: pk,
                    property_mappings: inner,
                    source_information: self.span(paren_tok, close),
                    embedded_class: None,
                },
            )));
            return self.entry_comma(&[T::BRACE_CLOSE, T::PAREN_CLOSE]);
        }
        let mut colon_tok = self.pos;
        self.expect(T::COLON)?;
        if local {
            let ty = unquote_path_iae(&self.parse_qualified_name()?)?;
            let (lower, upper) = self.parse_long_mult_bounds()?;
            let m_close = self.pos;
            self.expect(T::BRACKET_CLOSE)?;
            local_prop = Some(Box::new(PLocalProp {
                r#type: JStr(ty),
                lower_bound: lower,
                upper_bound: upper,
                source_information: self.span(colon_tok, m_close),
            }));
            colon_tok = self.pos;
            self.expect(T::COLON)?;
        }
        let enum_id = self.enumeration_mapping_id()?;
        let binding_id = self.binding_transformer_id()?;
        let op = self.parse_op_in_ctx(scope_db, scope)?;
        let has_local = local_prop.is_some();
        props.push(PPropertyMapping::PRelPropertyMapping(Box::new(PRelPropertyMapping {
            owner_class: if has_local { None } else { owner() },
            binding_transformer: binding_id,
            property: JStr(prop),
            property_source_information: boxed(prop_span),
            enum_mapping_id: enum_id,
            local_mapping_property: local_prop,
            relational_operation: op,
            source: src_id,
            target: if has_local { None } else { tgt_id },
            source_information: self.span(colon_tok, self.pos - 1),
        })));
        self.entry_comma(&[T::BRACE_CLOSE, T::PAREN_CLOSE])
    }

    /// `parseScopeBlock`: `scope([db]seg(.seg)?) ( prop-lines )`, flattened -- each line a plain property mapping
    /// whose table, when the header has one, spans the header's tokens.
    fn parse_scope_block(
        &mut self,
        props: &mut Vec<PPropertyMapping>,
        target: Option<&[u16]>,
        id: Option<&JStr>,
    ) -> R<()> {
        self.advance()?;
        self.expect(T::PAREN_OPEN)?;
        self.expect(T::BRACKET_OPEN)?;
        let db = unquote_path_iae(&self.parse_qualified_name()?)?;
        self.expect(T::BRACKET_CLOSE)?;
        let mut scope = None;
        if self.peek() != T::PAREN_CLOSE {
            let g1 = self.pos;
            let seg1 = self.parse_identifier()?;
            if self.peek() == T::DOT {
                self.advance()?;
                self.parse_identifier()?;
                scope = Some(ScopeCtx {
                    db: db.clone(),
                    schema: seg1,
                    table: self.tokens.text(self.pos - 1).to_vec(),
                    table_span: self.span_of(g1, self.pos - 1),
                    single_seg: false,
                });
            } else {
                scope = Some(ScopeCtx {
                    db: db.clone(),
                    schema: u("default"),
                    table: seg1,
                    table_span: self.span_of(g1, g1),
                    single_seg: true,
                });
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        self.expect(T::PAREN_OPEN)?;
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let scope_db = if scope.is_none() { Some(db.as_slice()) } else { None };
            self.parse_property_line(props, target, id, scope_db, scope.as_ref())?;
        }
        self.expect(T::PAREN_CLOSE)
    }

    /// `withSpanStart(op, startTok)`: the op with its span's start moved to `start_tok`'s.
    fn with_span_start(&self, op: PRelOp, start_tok: usize) -> PRelOp {
        let st = self.span_of(start_tok, start_tok);
        let w = span_from(&st, &op);
        with_rel_op_span(op, w)
    }

    /// `parseModelJoin`: `{ {x: T[1], y: U[1] | expr} }`, the inner braces one typed lambda.
    fn parse_model_join(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
    ) -> R<PModelJoinAssociationMapping> {
        self.expect(T::BRACE_OPEN)?;
        let l_s = self.pos;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if t == T::BRACE_OPEN {
                depth += 1;
            } else if t == T::BRACE_CLOSE {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            self.advance()?;
        }
        let mut body = parse_code_block_tokens(&self.tokens.slice(l_s, self.pos), JStr::default(), self.dialect)?;
        if body.len() != 1 {
            return Err(self.error(jcat!("ModelJoin body must be ONE lambda, got ", body.len())));
        }
        if !matches!(body[0], ValueSpecification::LambdaFunction(_)) {
            return Err(self.error_str(
                "ModelJoin association mapping requires a lambda join condition of the form '{src: SrcClass[1], tgt: \
                 TgtClass[1] | <boolean expression>}'",
            ));
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PModelJoinAssociationMapping {
            association: pointer("ASSOCIATION", target, target_span),
            id,
            join_condition: body.remove(0),
            source_information: self.span(member_start, close),
        })
    }

    /// `parseRelationClassMapping`: `{ ~func <descriptor> | ~src <descriptor> | ~src <expression>` then the
    /// primary key and the property lines -- the descriptor the canonical text of its tokens, a source lambda's
    /// wrapper span shifted down by its line less the brace's.
    fn parse_relation_class_mapping(
        &mut self,
        target: &[u16],
        member_start: usize,
        _target_span: &SourceInfo,
        id: Option<JStr>,
        extends_id: Option<JStr>,
        root: bool,
    ) -> R<PClassMappingRelation> {
        let brace_tok = self.pos;
        self.expect(T::BRACE_OPEN)?;
        let brace_line = self.tokens.start_line(brace_tok) as i64;
        let mut src_form = false;
        if self.peek() == T::SRC_CMD {
            self.advance()?;
            src_form = true;
            if !is_identifier_token(self.peek()) {
                let e_s = self.pos;
                let mut depth = 0i64;
                while !self.at_end() {
                    let t = self.peek();
                    if matches!(t, T::BRACE_OPEN | T::PAREN_OPEN | T::BRACKET_OPEN) {
                        depth += 1;
                    } else if matches!(t, T::PAREN_CLOSE | T::BRACKET_CLOSE) {
                        depth -= 1;
                    } else if depth == 0
                        && (t == T::BRACE_CLOSE
                            || t == T::PRIMARY_KEY_CMD
                            || (is_identifier_token(t) && self.peek_at(1) == T::COLON))
                    {
                        break;
                    } else if t == T::BRACE_CLOSE {
                        depth -= 1;
                    }
                    self.advance()?;
                }
                if e_s >= self.tokens.count() {
                    return Err(end_of_embedded_lambda(
                        self.tokens,
                        "~src expression must be ONE expression",
                    ));
                }
                let delta = self.tokens.start_line(e_s) as i64 - brace_line;
                let mut e_body =
                    parse_code_block_tokens(&self.tokens.slice(e_s, self.pos), JStr::default(), self.dialect)?;
                if e_body.len() != 1 {
                    return Err(self.error_str("~src expression must be ONE expression"));
                }
                let span = shift_lines(&self.span_of(e_s, self.pos - 1), delta);
                let src_lambda = PRelationSrcLambda {
                    function: None,
                    function_source_information: None,
                    expr: Some(e_body.remove(0)),
                    source_information: boxed(span),
                };
                return self.finish_relation_class_mapping(
                    target,
                    id,
                    extends_id,
                    root,
                    member_start,
                    brace_line,
                    Some(src_lambda),
                    None,
                    None,
                );
            }
        } else if self.peek() == T::TILDE && eq(self.peek_text(1), "func") {
            self.advance()?;
            self.advance()?;
        } else {
            return Err(self.error_str("Relation class mapping must open with ~func (or its ~src alias)"));
        }
        let fd = self.parse_function_descriptor()?;
        let desc = self.compact_text(fd.start, fd.end - 1);
        let fn_span = self.span_of(fd.start, fd.end - 1);
        let mut src_lambda = None;
        if src_form {
            let delta = self.tokens.start_line(fd.start) as i64 - brace_line;
            src_lambda = Some(PRelationSrcLambda {
                function: Some(JStr(self.compact_text(fd.start, fd.name_end - 1))),
                function_source_information: self.span(fd.start, fd.name_end - 1),
                expr: None,
                source_information: boxed(shift_lines(&fn_span, delta)),
            });
        }
        self.finish_relation_class_mapping(
            target,
            id,
            extends_id,
            root,
            member_start,
            brace_line,
            src_lambda,
            if src_form { None } else { Some(JStr(desc)) },
            if src_form { None } else { Some(fn_span) },
        )
    }

    /// `finishRelationClassMapping`: `~primaryKey: COL | [COL, ...]`, the property lines (plain, `+local`, embedded
    /// `prop ( nested )`, inline `prop () Inline [set]`) and the close.
    #[allow(clippy::too_many_arguments)]
    fn finish_relation_class_mapping(
        &mut self,
        target: &[u16],
        id: Option<JStr>,
        extends_id: Option<JStr>,
        root: bool,
        member_start: usize,
        brace_line: i64,
        src_lambda: Option<PRelationSrcLambda>,
        desc: Option<JStr>,
        fn_span: Option<SourceInfo>,
    ) -> R<PClassMappingRelation> {
        let mut pk = Vec::new();
        if self.peek() == T::PRIMARY_KEY_CMD {
            self.advance()?;
            self.expect(T::COLON)?;
            if self.match_tok(T::BRACKET_OPEN)? {
                while self.peek() != T::BRACKET_CLOSE && !self.at_end() {
                    pk.push(JStr(self.parse_identifier()?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
            } else {
                pk.push(JStr(self.parse_identifier()?));
            }
        }
        let owner = || Some(JStr(target.to_vec()));
        let mut props = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let p_s = self.pos;
            let local = self.match_tok(T::PLUS)?;
            let ident_tok = self.pos;
            let prop = self.parse_identifier()?;
            let prop_span = self.span_of(ident_tok, self.pos - 1);
            if self.peek() == T::PAREN_OPEN && self.peek_at(1) != T::PAREN_CLOSE {
                self.advance()?;
                let mut nested = Vec::new();
                while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                    let n_s = self.pos;
                    let n_prop = self.parse_identifier()?;
                    let n_span = self.span_of(n_s, self.pos - 1);
                    self.expect(T::COLON)?;
                    let n_enum = self.enumeration_mapping_id()?;
                    let n_binding = self.binding_transformer_id()?;
                    let nb = self.relation_binding(T::PAREN_CLOSE)?;
                    let wrapper = expr_wrapper(&nb, brace_line);
                    nested.push(Box::new(PRelationFnPropertyMapping {
                        owner_class: None,
                        binding_transformer: n_binding,
                        property: JStr(n_prop),
                        property_source_information: boxed(n_span),
                        column: nb.column,
                        inline_set_id: None,
                        nested: None,
                        local_mapping_property: None,
                        source: None,
                        enum_mapping_id: n_enum,
                        expr: nb.expr,
                        expr_lambda_source_information: wrapper,
                        source_information: self.span(n_s, self.pos - 1),
                    }));
                    self.match_tok(T::COMMA)?;
                }
                let p_close = self.pos;
                self.expect(T::PAREN_CLOSE)?;
                props.push(Box::new(PRelationFnPropertyMapping {
                    owner_class: owner(),
                    binding_transformer: None,
                    property: JStr(prop),
                    property_source_information: boxed(prop_span),
                    column: None,
                    inline_set_id: None,
                    nested: Some(nested),
                    local_mapping_property: None,
                    source: id.clone(),
                    enum_mapping_id: None,
                    expr: None,
                    expr_lambda_source_information: None,
                    source_information: self.span(p_s, p_close),
                }));
                self.match_tok(T::COMMA)?;
                continue;
            }
            if self.peek() == T::PAREN_OPEN {
                self.advance()?;
                self.expect(T::PAREN_CLOSE)?;
                if self.peek() != T::INLINE {
                    return Err(self.error_str("expected Inline after ()"));
                }
                self.advance()?;
                self.expect(T::BRACKET_OPEN)?;
                let set_id = self.parse_set_id()?;
                let b_close = self.pos;
                self.expect(T::BRACKET_CLOSE)?;
                props.push(Box::new(PRelationFnPropertyMapping {
                    owner_class: owner(),
                    binding_transformer: None,
                    property: JStr(prop),
                    property_source_information: boxed(prop_span),
                    column: None,
                    inline_set_id: Some(JStr(set_id)),
                    nested: None,
                    local_mapping_property: None,
                    source: id.clone(),
                    enum_mapping_id: None,
                    expr: None,
                    expr_lambda_source_information: None,
                    source_information: self.span(p_s, b_close),
                }));
                self.match_tok(T::COMMA)?;
                continue;
            }
            self.expect(T::COLON)?;
            let mut lp = None;
            if local {
                let ty = unquote_path_iae(&self.parse_qualified_name()?)?;
                let (lower, upper) = self.parse_long_mult_bounds()?;
                self.expect(T::BRACKET_CLOSE)?;
                lp = Some(Box::new(PLocalProp {
                    r#type: JStr(ty),
                    lower_bound: lower,
                    upper_bound: upper,
                    source_information: boxed(prop_span.clone()),
                }));
                self.expect(T::COLON)?;
            }
            let enum_id = self.enumeration_mapping_id()?;
            let binding_id = self.binding_transformer_id()?;
            let rb = self.relation_binding(T::BRACE_CLOSE)?;
            let wrapper = expr_wrapper(&rb, brace_line);
            props.push(Box::new(PRelationFnPropertyMapping {
                owner_class: owner(),
                binding_transformer: binding_id,
                property: JStr(prop),
                property_source_information: boxed(prop_span),
                column: rb.column,
                inline_set_id: None,
                nested: None,
                local_mapping_property: lp,
                source: id.clone(),
                enum_mapping_id: enum_id,
                expr: rb.expr,
                expr_lambda_source_information: wrapper,
                source_information: self.span(p_s, self.pos - 1),
            }));
            self.match_tok(T::COMMA)?;
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PClassMappingRelation {
            class_name: JStr(target.to_vec()),
            id,
            extends_class_mapping_id: extends_id,
            primary_key: pk,
            property_mappings: props,
            relation_function: desc,
            relation_function_source_information: fn_span.map(Box::new),
            source_lambda: src_lambda.map(Box::new),
            root,
            source_information: self.span(member_start, close),
        })
    }

    /// `parseAggregationAware`: `{ Views: [ ( ~modelOperation: {...}, ~aggregateMapping: Relational|Pure {...} ) ],
    /// ~mainMapping: Relational|Pure {...} }` -- the nested class mappings parsed after, from the original stream,
    /// their class and own spans the outer ones shifted down by the member's brace line less the section's first
    /// content line plus one.
    fn parse_aggregation_aware(
        &mut self,
        target: &[u16],
        member_start: usize,
        target_span: &SourceInfo,
        id: Option<JStr>,
        root: bool,
        extends_id: Option<JStr>,
    ) -> R<PClassMappingAggregationAware> {
        if self.section_start_line < 0 {
            return Err(self.error_str("AggregationAware needs the section start line (span-shift emulation)"));
        }
        let outer_id = set_id_of(id.as_ref(), target);
        let brace_open_tok = self.pos;
        self.expect(T::BRACE_OPEN)?;
        let mut can_aggs = Vec::new();
        let mut group_bys = Vec::new();
        let mut agg_vals = Vec::new();
        let mut agg_body_toks: Vec<isize> = Vec::new();
        let mut main_body_tok: isize = 0;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            if self.peek() == T::VALID_STRING && eq(self.text(), "Views") {
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    self.expect(T::PAREN_OPEN)?;
                    let tilde_tok = self.pos;
                    self.expect(T::TILDE)?;
                    self.expect_text("modelOperation")?;
                    self.expect(T::COLON)?;
                    let brace_tok = self.pos;
                    self.expect(T::BRACE_OPEN)?;
                    self.agg_lambda_shift =
                        self.tokens.start_line(brace_tok) as i64 - self.tokens.start_line(tilde_tok) as i64;
                    let mut can_agg = false;
                    let mut gbs = Vec::new();
                    let mut avs = Vec::new();
                    while !self.at_end() && self.peek() != T::BRACE_CLOSE {
                        self.expect(T::TILDE)?;
                        let cmd = self.parse_identifier()?;
                        if eq(&cmd, "canAggregate") {
                            can_agg = parse_boolean(self.slack_text());
                            self.advance()?;
                        } else if eq(&cmd, "groupByFunctions") {
                            self.expect(T::PAREN_OPEN)?;
                            while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                                gbs.push(self.parse_agg_lambda()?);
                                self.match_tok(T::COMMA)?;
                            }
                            self.expect(T::PAREN_CLOSE)?;
                        } else if eq(&cmd, "aggregateValues") {
                            self.expect(T::PAREN_OPEN)?;
                            while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                                self.expect(T::PAREN_OPEN)?;
                                self.expect(T::TILDE)?;
                                self.expect_text("mapFn")?;
                                self.expect(T::COLON)?;
                                let map_fn = self.parse_agg_lambda()?;
                                self.expect(T::COMMA)?;
                                self.expect(T::TILDE)?;
                                self.expect_text("aggregateFn")?;
                                self.expect(T::COLON)?;
                                let aggregate_fn = self.parse_agg_lambda()?;
                                self.expect(T::PAREN_CLOSE)?;
                                avs.push(Box::new(PAggregateValue { map_fn, aggregate_fn }));
                                self.match_tok(T::COMMA)?;
                            }
                            self.expect(T::PAREN_CLOSE)?;
                        } else {
                            return Err(self.error(jcat!("modelOperation command '~", cmd, "' is unbuilt")));
                        }
                        self.match_tok(T::COMMA)?;
                    }
                    self.expect(T::BRACE_CLOSE)?;
                    self.match_tok(T::COMMA)?;
                    self.expect(T::TILDE)?;
                    self.expect_text("aggregateMapping")?;
                    self.expect(T::COLON)?;
                    let agg_is_pure = self.peek() == T::PURE_MAPPING;
                    if agg_is_pure {
                        self.advance()?;
                    } else {
                        self.expect(T::RELATIONAL)?;
                    }
                    can_aggs.push(can_agg);
                    group_bys.push(gbs);
                    agg_vals.push(avs);
                    agg_body_toks.push(if agg_is_pure {
                        -(self.pos as isize)
                    } else {
                        self.pos as isize
                    });
                    self.skip_brace_block()?;
                    self.expect(T::PAREN_CLOSE)?;
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                self.match_tok(T::COMMA)?;
                continue;
            }
            if self.peek() == T::TILDE && eq(self.peek_text(1), "mainMapping") {
                self.advance()?;
                self.advance()?;
                self.expect(T::COLON)?;
                let main_is_pure = self.peek() == T::PURE_MAPPING;
                if main_is_pure {
                    self.advance()?;
                } else {
                    self.expect(T::RELATIONAL)?;
                }
                main_body_tok = if main_is_pure {
                    -(self.pos as isize)
                } else {
                    self.pos as isize
                };
                self.skip_brace_block()?;
                self.match_tok(T::COMMA)?;
                continue;
            }
            return Err(self.error(jcat!("AggregationAware member '", self.safe_text(), "' is unbuilt")));
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        if main_body_tok == 0 {
            return Err(self.error_str("AggregationAware without ~mainMapping"));
        }
        let delta = self.tokens.start_line(brace_open_tok) as i64 - self.section_start_line + 1;
        let shifted_target = shift_lines(target_span, delta);
        let shifted_member = shift_lines(&self.span_of(member_start, close), delta);
        let mut asis = Vec::new();
        for (i, body_tok) in agg_body_toks.iter().enumerate() {
            let mut cm_id = outer_id.clone();
            cm_id.extend(jcat!("_Aggregate_", i).0);
            let cm = self.parse_nested(
                *body_tok,
                target,
                &shifted_target,
                cm_id,
                id.clone(),
                root,
                &shifted_member,
                extends_id.clone(),
            )?;
            asis.push(Box::new(PAggregateSetImplementation {
                can_aggregate: can_aggs[i],
                group_by_functions: std::mem::take(&mut group_bys[i]),
                aggregate_values: std::mem::take(&mut agg_vals[i]),
                index: i as i32,
                set_implementation: cm,
            }));
        }
        let mut main_id = outer_id.clone();
        main_id.extend(u("_Main"));
        let main_cm = self.parse_nested(
            main_body_tok,
            target,
            &shifted_target,
            main_id,
            id,
            root,
            &shifted_member,
            extends_id.clone(),
        )?;
        let mut agg_pms = Vec::new();
        if let PClassMapping::PClassMappingPure(pu) = &main_cm {
            for pm in &pu.property_mappings {
                agg_pms.push(Box::new(PPurePropertyMapping {
                    owner_class: pm.owner_class.clone(),
                    property: pm.property.clone(),
                    property_source_information: pm.property_source_information.clone(),
                    enum_mapping_id: None,
                    explode_property: false,
                    local_mapping_property: None,
                    transform: Vec::new(),
                    source: pm.source.clone(),
                    target: pm.target.clone(),
                    source_information: pm.source_information.clone(),
                }));
            }
        }
        Ok(PClassMappingAggregationAware {
            class_name: JStr(target.to_vec()),
            id: JStr(outer_id),
            aggregate_set_implementations: asis,
            main_set_implementation: main_cm,
            agg_property_mappings: agg_pms,
            root,
            source_information: self.span(member_start, close),
            extends_class_mapping_id: extends_id,
        })
    }

    /// `parseNested`: a nested set implementation parsed from the original stream at its body (a negative token a
    /// Pure body), against the outer header; its class and own spans the shifted outer ones, its id `cm_id`.
    #[allow(clippy::too_many_arguments)]
    fn parse_nested(
        &self,
        signed_body_tok: isize,
        target: &[u16],
        shifted_target: &SourceInfo,
        cm_id: Vec<u16>,
        explicit_outer_id: Option<JStr>,
        root: bool,
        shifted_member: &SourceInfo,
        extends_id: Option<JStr>,
    ) -> R<PClassMapping> {
        if signed_body_tok < 0 {
            let body_tok = (-signed_body_tok) as usize;
            let mut p = MappingCursor::at(self.tokens, body_tok, self.dialect);
            let cm =
                p.parse_pure_class_mapping(target, body_tok, shifted_target, explicit_outer_id, root, extends_id)?;
            return Ok(PClassMapping::PClassMappingPure(Box::new(PClassMappingPure {
                class_name: cm.class_name,
                class_source_information: boxed(shifted_target.clone()),
                extends_class_mapping_id: cm.extends_class_mapping_id,
                id: Some(JStr(cm_id)),
                root: cm.root,
                src_class: cm.src_class,
                source_class_source_information: cm.source_class_source_information,
                filter: cm.filter,
                property_mappings: cm.property_mappings,
                source_information: boxed(shifted_member.clone()),
            })));
        }
        let body_tok = signed_body_tok as usize;
        let mut p = MappingCursor::at(self.tokens, body_tok, self.dialect);
        // the property mappings' sources keep the explicit outer id: the engine suffixes the id after the sub-parse
        let cm =
            p.parse_relational_class_mapping(target, body_tok, shifted_target, explicit_outer_id, root, extends_id)?;
        Ok(PClassMapping::PClassMappingRel(Box::new(PClassMappingRel {
            class_name: cm.class_name,
            class_source_information: boxed(shifted_target.clone()),
            id: Some(JStr(cm_id)),
            root: cm.root,
            distinct: cm.distinct,
            extends_class_mapping_id: cm.extends_class_mapping_id,
            filter: cm.filter,
            group_by: cm.group_by,
            main_table: cm.main_table,
            primary_key: cm.primary_key,
            property_mappings: cm.property_mappings,
            source_information: boxed(shifted_member.clone()),
        })))
    }

    /// `parseAggLambda`: one aggregate lambda up to a top-level `,` or `)`, re-lexed padded to its line less the
    /// current `~modelOperation` shift and to its column.
    fn parse_agg_lambda(&mut self) -> R<ValueSpecification> {
        let e_s = self.pos;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if depth == 0 && (t == T::COMMA || t == T::PAREN_CLOSE) {
                break;
            }
            if is_open(t) {
                depth += 1;
            } else if is_close(t) {
                depth -= 1;
            }
            self.advance()?;
        }
        let ts = self.tokens;
        if e_s >= ts.count() {
            return Err(end_of_embedded_lambda(ts, "aggregate lambda must be ONE expression"));
        }
        let start_line = ts.start_line(e_s) as i64;
        let mut padded: Vec<u16> = Vec::new();
        let mut i = 1;
        while i < start_line - self.agg_lambda_shift {
            padded.push(u16::from(b'\n'));
            i += 1;
        }
        for _ in 1..ts.start_column(e_s) {
            padded.push(u16::from(b' '));
        }
        let (from, to) = (ts.start(e_s), if self.pos == 0 { 0 } else { ts.end(self.pos - 1) });
        if from > to {
            // an empty lambda with blanks before its `,` or `)`: refused at that token, as the engine does (PARITY row 32)
            return Err(self.error_str("aggregate lambda must be ONE expression").as_refusal());
        }
        padded.extend_from_slice(&ts.source()[from..to]);
        let mut body = parse_code_block(&padded, self.dialect)?;
        if body.len() != 1 {
            return Err(self.error_str("aggregate lambda must be ONE expression"));
        }
        Ok(body.remove(0))
    }

    /// `parseMaybeQuotedIdentifier`: a `"quoted"` table spelling keeps its quotes.
    fn parse_maybe_quoted_identifier(&mut self) -> R<Vec<u16>> {
        if self.peek() == T::QUOTED_STRING {
            let raw = self.text().to_vec();
            self.advance()?;
            return Ok(raw);
        }
        self.parse_identifier()
    }

    /// `skipBraceBlock`: a brace-balanced block from its `{`.
    fn skip_brace_block(&mut self) -> R<()> {
        self.expect(T::BRACE_OPEN)?;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if t == T::BRACE_OPEN {
                depth += 1;
            } else if t == T::BRACE_CLOSE {
                if depth == 0 {
                    self.advance()?;
                    return Ok(());
                }
                depth -= 1;
            }
            self.advance()?;
        }
        Err(self.error_str("unclosed brace block"))
    }

    /// `parseOpInCtx`: one embedded operation under the active scope -- its table, else its database, else bare.
    fn parse_op_in_ctx(&mut self, scope_db: Option<&[u16]>, scope: Option<&ScopeCtx>) -> R<PRelOp> {
        let (op, pos) = if let Some(scope) = scope {
            scoped_operation_at(self.tokens, self.pos, scope, self.dialect)?
        } else if let Some(db) = scope_db {
            operation_at(self.tokens, self.pos, db, &u("default"), self.dialect)?
        } else {
            return self.parse_embedded_operation();
        };
        self.pos = pos;
        Ok(op)
    }

    /// `parseEmbeddedOperation`: one relational operation by the Database grammar, outside any database.
    fn parse_embedded_operation(&mut self) -> R<PRelOp> {
        let (op, pos) = operation_at(self.tokens, self.pos, &[], &u("default"), self.dialect)?;
        self.pos = pos;
        Ok(op)
    }

    /// `entryComma(closers)`: a comma promises another entry -- a closer right after one refused.
    fn entry_comma(&mut self, closers: &[TokenType]) -> R<()> {
        if !self.match_tok(T::COMMA)? {
            return Ok(());
        }
        if closers.contains(&self.peek()) {
            return Err(self.error_str("trailing comma: a comma must be followed by another entry"));
        }
        Ok(())
    }

    /// `parseEnumerationMapping`: `path: EnumerationMapping id? { V: [src, ...] | src, ... }`, its span the target
    /// path (or the root marker) through the close.
    fn parse_enumeration_mapping(
        &mut self,
        target: &[u16],
        target_start: usize,
        target_span: &SourceInfo,
    ) -> R<PEnumerationMapping> {
        let id = if self.peek() == T::BRACE_OPEN {
            None
        } else {
            Some(JStr(self.parse_mapping_word()?))
        };
        self.expect(T::BRACE_OPEN)?;
        let mut rows = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let enum_value = self.parse_identifier()?;
            self.expect(T::COLON)?;
            let mut sources = Vec::new();
            if self.peek() == T::BRACKET_OPEN {
                self.advance()?;
                while self.peek() != T::BRACKET_CLOSE && !self.at_end() {
                    sources.push(Box::new(self.parse_source_value()?));
                    self.entry_comma(&[T::BRACKET_CLOSE])?;
                }
                if sources.is_empty() {
                    return Err(self.error(jcat!(
                        "enum value '",
                        enum_value,
                        "' must list at least one source value"
                    )));
                }
                self.expect(T::BRACKET_CLOSE)?;
            } else {
                sources.push(Box::new(self.parse_source_value()?));
            }
            rows.push(Box::new(PEnumValueMapping {
                enum_value: JStr(enum_value),
                source_values: sources,
            }));
            self.entry_comma(&[T::BRACE_CLOSE])?;
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PEnumerationMapping {
            id,
            enumeration: pointer("ENUMERATION", target, target_span),
            enum_value_mappings: rows,
            source_information: self.span(target_start, close),
        })
    }

    /// `parseLongMultBounds`: a local property's `[m]` bounds as the engine's longs, stopping before the `]`.
    fn parse_long_mult_bounds(&mut self) -> R<(i64, Option<i64>)> {
        self.expect(T::BRACKET_OPEN)?;
        let (lower, mut upper) = if self.peek() == T::STAR {
            self.advance()?;
            (0, None)
        } else {
            let l = self.consume_long()?;
            (l, Some(l))
        };
        if self.peek() == T::DOT_DOT {
            self.advance()?;
            if self.peek() == T::STAR {
                self.advance()?;
                upper = None;
            } else {
                upper = Some(self.consume_long()?);
            }
        }
        Ok((lower, upper))
    }

    /// `parseSourceValue`: an enumeration mapping's source -- a string, an integer, or an enum value `path.V`.
    fn parse_source_value(&mut self) -> R<PEnumSourceValue> {
        if self.peek() == T::STRING {
            let v = self.unquote_and_unescape(self.text())?;
            self.advance()?;
            return Ok(PEnumSourceValue {
                enumeration: None,
                value: JValue::String(JStr(v)),
            });
        }
        if self.peek() == T::INTEGER {
            return Ok(PEnumSourceValue {
                enumeration: None,
                value: JValue::Long(self.consume_long()?),
            });
        }
        if is_identifier_token(self.peek()) {
            let enum_path = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::DOT)?;
            let v = if self.peek() == T::STRING {
                let v = self.unquote_and_unescape(self.text())?;
                self.advance()?;
                v
            } else {
                self.parse_identifier()?
            };
            return Ok(PEnumSourceValue {
                enumeration: Some(JStr(enum_path)),
                value: JValue::String(JStr(v)),
            });
        }
        Err(self.error(jcat!("unsupported enum source value: ", self.safe_text())))
    }
}

/// `new PPathOffset(startType, propertyPath)`: `$service.response`, its segments, one empty argument list each.
fn path_offset_of(segs: Vec<JStr>) -> PPathOffset {
    let parameters = segs.iter().map(|_| Vec::new()).collect();
    PPathOffset {
        start_type: JStr::from("$service.response"),
        property_path: segs,
        parameters,
    }
}

/// `Boolean.parseBoolean`: `"true"`, ignoring case.
fn parse_boolean(s: &[u16]) -> bool {
    s.len() == 4
        && s.iter()
            .zip("true".bytes())
            .all(|(&c, b)| c < 128 && (c as u8).eq_ignore_ascii_case(&b))
}

/// An embedded lambda (`~src`, an aggregate's) cut off by the end of the input: the engine re-parses it on its own, and
/// its parser's `<EOF>` lands one column before the end of the text (PARITY rows 29 and 32).
fn end_of_embedded_lambda(tokens: &TokenStream<'_>, message: &str) -> JavaError {
    let (line, column) = crate::cursor::eof_place(tokens);
    JavaError::parse(JStr::from(message), line, column - 1).as_refusal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn a_relational_class_mapping_reads_its_clauses_and_properties() {
        let text = u(
            "Mapping m::M (\n  *m::P[p]: Relational { ~mainTable [db::D] T name: [db::D] T.NAME, age: \
                      [db::D] T.AGE }\n)",
        );
        let ts = tokenize(&text);
        let (m, end) = parse_mapping(&ts, 0, 2, Dialect::LegendLite).expect("parses");
        assert_eq!(end, ts.count());
        let PClassMapping::PClassMappingRel(cm) = &m.class_mappings[0] else {
            panic!("relational")
        };
        assert!(cm.root && cm.main_table.is_some());
        assert_eq!(cm.property_mappings.len(), 2);
    }

    #[test]
    fn a_clause_after_a_property_is_refused() {
        let text = u("Mapping m::M ( m::P: Relational { name: [db::D] T.NAME ~distinct } )");
        let ts = tokenize(&text);
        let e = parse_mapping(&ts, 0, 1, Dialect::LegendLite).unwrap_err();
        assert!(e.message.unwrap().to_string().contains("~distinct is out of order"));
    }
}
