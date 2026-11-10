// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###DataQualityValidation` (core/'s `parser.section.DataQualityValidationSectionGrammar`, docs/rust-port/PHASE2.md §4
//! step 4d): `DataQualityValidation` (a context, a `$[...]$` validation tree, a filter), `DataQualityRelationValidation`
//! (a query and named assertions, test suites) and `DataQualityRelationComparison` (source and target lambdas, keys, a
//! reconciliation strategy, test suites); every lambda through the expression parser, the suites' data and assertions
//! through the Data and Mapping readers. Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, once, once_at, throw_at, unquote_path_iae};
use crate::jcat;
use crate::jnum::parse_double;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{
    Element, PDataQualityRelationComparison, PDataQualityRelationValidation, PDataQualityValidation, PDqRelationCheck,
    PDqStoreData, PDqTest, PDqTestData, PDqTestSuite, PDqTreeNode, PReconStrategy, ValueSpecification,
};
use crate::sections::{head, lambda_to_semicolon, spec_to_semicolon, string_value};
use crate::service_parser::{embedded_value_at, test_assertion_at};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

/// `DataQualityValidationSectionGrammar.parseOne` and `qualifiedNameOf`.
pub fn parse_element<'s>(c: &mut impl Cursor<'s>) -> R<(Element, Vec<u16>)> {
    let kind = c.safe_text();
    let qn = crate::protocol::qualified_name;
    if eq(&kind, "DataQualityValidation") {
        let v = parse_validation(c)?;
        let path = qn(&v.pkg.0, &v.name.0);
        return Ok((Element::PDataQualityValidation(Box::new(v)), path));
    }
    if eq(&kind, "DataQualityRelationValidation") {
        let v = parse_relation_validation(c)?;
        let path = qn(&v.pkg.0, &v.name.0);
        return Ok((Element::PDataQualityRelationValidation(Box::new(v)), path));
    }
    if eq(&kind, "DataQualityRelationComparison") {
        let v = parse_relation_comparison(c)?;
        let path = qn(&v.pkg.0, &v.name.0);
        return Ok((Element::PDataQualityRelationComparison(Box::new(v)), path));
    }
    Err(c.error(jcat!("unsupported ###DataQualityValidation element: ", kind)))
}

/// `parseValidation`: `'''doc''' DataQualityValidation <<..>> qn { context: kind(qn, second); validationTree: $[...]$;
/// filter: expr; }`, each key once but the filter (its first kept).
fn parse_validation<'s>(c: &mut impl Cursor<'s>) -> R<PDataQualityValidation> {
    let h = head(c, "DataQualityValidation", true)?;
    c.expect(T::BRACE_OPEN)?;
    let mut ctx_kind = None;
    let mut ctx_path = None;
    let mut ctx_path_span = None;
    let mut ctx_second = None;
    let mut ctx_second_span = None;
    let mut tree = None;
    let mut filter: Option<ValueSpecification> = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        if !eq(&key, "filter") {
            once(&mut seen, &key, c)?;
        }
        c.expect(T::COLON)?;
        if eq(&key, "context") {
            let kind = c.parse_identifier()?;
            c.expect(T::PAREN_OPEN)?;
            let s = c.pos();
            ctx_path = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            ctx_path_span = c.span(s, c.pos() - 1);
            if c.match_tok(T::COMMA)? {
                let s2 = c.pos();
                let second = if c.peek() == T::STRING {
                    string_value(c)?
                } else {
                    unquote_path_iae(&c.parse_qualified_name()?)?
                };
                ctx_second = Some(JStr(second));
                if eq(&kind, "fromMappingAndRuntime") {
                    ctx_second_span = c.span(s2, c.pos() - 1);
                }
            }
            ctx_kind = Some(JStr(kind));
            c.expect(T::PAREN_CLOSE)?;
        } else if eq(&key, "validationTree") {
            tree = Some(parse_tree_island(c)?);
        } else if eq(&key, "filter") {
            let f2 = spec_to_semicolon(c)?;
            if filter.is_none() {
                filter = Some(f2);
            }
        } else {
            return Err(c.error(jcat!("unknown DataQualityValidation key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let (Some(ctx_kind), Some(ctx_path), Some(tree)) = (ctx_kind, ctx_path, tree) else {
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            JStr::from("DataQualityValidation needs context and validationTree"),
        ));
    };
    Ok(PDataQualityValidation {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        stereotypes: h.dec.stereotypes,
        tagged_values: h.dec.tagged_values,
        context_kind: ctx_kind,
        context_path: ctx_path,
        context_path_span: ctx_path_span,
        context_second: ctx_second,
        context_second_span: ctx_second_span,
        validation_tree: Box::new(tree),
        filter,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseTreeIsland`: `$[ Class<c1, 'c2'>{ prop, sub<c>{...} } ]$`.
fn parse_tree_island<'s>(c: &mut impl Cursor<'s>) -> R<PDqTreeNode> {
    c.expect(T::DOLLAR)?;
    c.expect(T::BRACKET_OPEN)?;
    let root = parse_tree_node(c, true)?;
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::DOLLAR)?;
    Ok(root)
}

/// `parseTreeNode`: a class (the root) or a property, its constraints, a `->subType(@qn)`, its sub-trees; the span the
/// name's alone.
fn parse_tree_node<'s>(c: &mut impl Cursor<'s>, root: bool) -> R<PDqTreeNode> {
    let s = c.pos();
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let span = c.span(s, c.pos() - 1);
    let mut constraints = Vec::new();
    if c.match_tok(T::LESS_THAN)? {
        while c.peek() != T::GREATER_THAN {
            let v = if c.peek() == T::STRING {
                string_value(c)?
            } else {
                c.parse_identifier()?
            };
            constraints.push(JStr(v));
            if !c.match_tok(T::COMMA)? {
                break;
            }
        }
        c.expect(T::GREATER_THAN)?;
    }
    let mut sub_type = None;
    if c.peek() == T::ARROW {
        c.advance()?;
        let stw = c.parse_identifier()?;
        if !eq(&stw, "subType") {
            return Err(c.error(jcat!("unknown tree arrow '", stw, "'")));
        }
        c.expect(T::PAREN_OPEN)?;
        c.expect(T::AT)?;
        sub_type = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
        c.expect(T::PAREN_CLOSE)?;
    }
    let mut sub_trees = Vec::new();
    if c.match_tok(T::BRACE_OPEN)? {
        while c.peek() != T::BRACE_CLOSE {
            sub_trees.push(Box::new(parse_tree_node(c, false)?));
            // `item (COMMA item)*` at the engine's dialect, as ruling 18 holds every comma list (ruling 20, register S169)
            if !c.match_list_comma()? {
                break;
            }
        }
        c.expect(T::BRACE_CLOSE)?;
    }
    let (class_name, property) = if root {
        (Some(JStr(qn)), None)
    } else {
        (None, Some(JStr(qn)))
    };
    Ok(PDqTreeNode {
        class_name,
        property,
        constraints,
        sub_trees,
        sub_type,
        source_information: span,
        alias: None,
        parameters: Vec::new(),
    })
}

/// `parseRelationValidation`: `'''doc''' DataQualityRelationValidation <<..>> qn { query: expr; validations: [...];
/// testSuites: [...] }`, each key once, `validations` and the query required.
fn parse_relation_validation<'s>(c: &mut impl Cursor<'s>) -> R<PDataQualityRelationValidation> {
    let h = head(c, "DataQualityRelationValidation", true)?;
    c.expect(T::BRACE_OPEN)?;
    let mut query = None;
    let mut validations = Vec::new();
    let mut test_suites = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if eq(&key, "query") {
            query = Some(lambda_to_semicolon(c)?);
        } else if eq(&key, "validations") {
            parse_checks(c, &mut validations, h.decl_start)?;
        } else if eq(&key, "testSuites") {
            test_suites = Some(parse_test_suites(c)?);
            continue;
        } else {
            return Err(c.error(jcat!("unknown DataQualityRelationValidation key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    if !seen.contains(&crate::ustr::u("validations")) {
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            JStr::from("Field 'validations' is required"),
        ));
    }
    let Some(query) = query else {
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            JStr::from("DataQualityRelationValidation needs a query"),
        ));
    };
    Ok(PDataQualityRelationValidation {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        stereotypes: h.dec.stereotypes,
        tagged_values: h.dec.tagged_values,
        query,
        validations,
        test_suites,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseTestSuites`: `[ id: { data: [ store: EmbeddedData, ... ] tests: [ id: { asserts: [ id: Assertion, ... ] },
/// ... ] }, ... ]` -- each suite's, test's and data entry's span its whole rule, `tests` and `asserts` required.
fn parse_test_suites<'s>(c: &mut impl Cursor<'s>) -> R<Vec<Box<PDqTestSuite>>> {
    let mut out = Vec::new();
    c.expect(T::BRACKET_OPEN)?;
    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
        let ss = c.pos();
        let id = c.parse_identifier()?;
        c.expect(T::COLON)?;
        c.expect(T::BRACE_OPEN)?;
        let mut data = None;
        let mut tests: Option<Vec<Box<PDqTest>>> = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            once_at(&mut seen, &key, c.tokens(), ss)?;
            c.expect(T::COLON)?;
            if eq(&key, "data") {
                let mut entries = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                    let es = c.pos();
                    let store = unquote_path_iae(&c.parse_qualified_name()?)?;
                    let store_span = c.span(es, c.pos() - 1);
                    c.expect(T::COLON)?;
                    let v = embedded_value_at(c)?;
                    entries.push(Box::new(PDqStoreData {
                        store: JStr(store),
                        store_span,
                        data: v,
                        source_information: c.span(es, c.pos() - 1),
                    }));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                data = Some(Box::new(PDqTestData {
                    test_data: entries,
                    source_information: c.span(key_start, c.pos() - 1),
                }));
            } else if eq(&key, "tests") {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                    let ts = c.pos();
                    let test_id = c.parse_identifier()?;
                    c.expect(T::COLON)?;
                    c.expect(T::BRACE_OPEN)?;
                    let mut asserts = None;
                    let mut seen_test: HashSet<Vec<u16>> = HashSet::new();
                    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
                        let tk = c.parse_identifier()?;
                        once_at(&mut seen_test, &tk, c.tokens(), ts)?;
                        c.expect(T::COLON)?;
                        if !eq(&tk, "asserts") {
                            return Err(c.error(jcat!("unknown test key '", tk, "'")));
                        }
                        let mut a = Vec::new();
                        c.expect(T::BRACKET_OPEN)?;
                        while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                            a.push(Box::new(test_assertion_at(c)?));
                            if !c.match_tok(T::COMMA)? {
                                break;
                            }
                        }
                        asserts = Some(a);
                        c.expect(T::BRACKET_CLOSE)?;
                    }
                    c.expect(T::BRACE_CLOSE)?;
                    let Some(assertions) = asserts else {
                        return Err(throw_at(c.tokens(), ts, JStr::from("Field 'asserts' is required")));
                    };
                    list.push(Box::new(PDqTest {
                        id: JStr(test_id),
                        assertions,
                        source_information: c.span(ts, c.pos() - 1),
                    }));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                tests = Some(list);
                c.expect(T::BRACKET_CLOSE)?;
            } else {
                return Err(c.error(jcat!("unknown testSuite key '", key, "'")));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let Some(tests) = tests else {
            return Err(throw_at(c.tokens(), ss, JStr::from("Field 'tests' is required")));
        };
        out.push(Box::new(PDqTestSuite {
            id: JStr(id),
            test_data: data,
            tests,
            source_information: c.span(ss, c.pos() - 1),
        }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)?;
    Ok(out)
}

/// `parseChecks`: `[ { name: '..'; description: '..'; assertion: expr; type: ROW_LEVEL; }, ... ]`, a name and an
/// assertion required (at the element).
fn parse_checks<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PDqRelationCheck>>, element_anchor: usize) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE {
        c.expect(T::BRACE_OPEN)?;
        let mut name = None;
        let mut description = None;
        let mut r#type = None;
        let mut assertion = None;
        while c.peek() != T::BRACE_CLOSE {
            let k = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if eq(&k, "name") {
                name = Some(JStr(string_value(c)?));
            } else if eq(&k, "description") {
                description = Some(JStr(string_value(c)?));
            } else if eq(&k, "assertion") {
                assertion = Some(spec_to_semicolon(c)?);
            } else if eq(&k, "type") {
                let t = c.parse_identifier()?;
                if !eq(&t, "ROW_LEVEL") && !eq(&t, "AGGREGATE") {
                    return Err(c.error(jcat!("unknown validation type '", t, "'")));
                }
                r#type = Some(JStr(t));
            } else {
                return Err(c.error(jcat!("unknown validation key '", k, "'")));
            }
            c.expect(T::SEMI_COLON)?;
        }
        c.expect(T::BRACE_CLOSE)?;
        let (Some(name), Some(assertion)) = (name, assertion) else {
            return Err(throw_at(
                c.tokens(),
                element_anchor,
                JStr::from("validation needs name and assertion"),
            ));
        };
        out.push(Box::new(PDqRelationCheck {
            name,
            description,
            assertion,
            r#type,
        }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parseRelationComparison`: `DataQualityRelationComparison qn { source: expr; target: expr; keys: [...];
/// strategy: S {...}; columnsToCompare: [...]; expectedMatch: n; testSuites: [...] }` (no documentation), each key once;
/// the source, target and strategy required.
fn parse_relation_comparison<'s>(c: &mut impl Cursor<'s>) -> R<PDataQualityRelationComparison> {
    let h = head(c, "DataQualityRelationComparison", false)?;
    c.expect(T::BRACE_OPEN)?;
    let mut source = None;
    let mut target = None;
    let mut keys = Vec::new();
    let mut columns_to_compare = Vec::new();
    let mut expected_match = None;
    let mut strategy = None;
    let mut test_suites = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if eq(&key, "source") {
            source = Some(lambda_to_semicolon(c)?);
        } else if eq(&key, "target") {
            target = Some(lambda_to_semicolon(c)?);
        } else if eq(&key, "keys") {
            identifier_list(c, &mut keys)?;
        } else if eq(&key, "strategy") {
            strategy = Some(parse_recon_strategy(c)?);
        } else if eq(&key, "columnsToCompare") {
            identifier_list(c, &mut columns_to_compare)?;
        } else if eq(&key, "expectedMatch") {
            let (at, shown) = (c.pos(), c.safe_text());
            let num = c.slack_text();
            c.advance()?;
            // the engine's grammar takes a number alone here, and refuses the token (PARITY row 49)
            expected_match = Some(
                parse_double(num)
                    .map_err(|_| crate::cursor::refused_at(c.tokens(), at, jcat!("Unexpected token '", shown, "'")))?,
            );
        } else if eq(&key, "testSuites") {
            test_suites = Some(parse_test_suites(c)?);
            continue;
        } else {
            return Err(c.error(jcat!("unknown DataQualityRelationComparison key '", key, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let (Some(source), Some(target), Some(strategy)) = (source, target, strategy) else {
        return Err(throw_at(
            c.tokens(),
            h.decl_start,
            JStr::from("DataQualityRelationComparison needs source, target and strategy"),
        ));
    };
    Ok(PDataQualityRelationComparison {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        source,
        target,
        keys,
        columns_to_compare,
        expected_match,
        strategy: Box::new(strategy),
        test_suites,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `[ id, ... ]`.
fn identifier_list<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<JStr>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE {
        out.push(JStr(c.parse_identifier()?));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parseReconStrategy`: `MD5Hash ( { sourceHashColumn: x; targetHashColumn: y; aggregatedHash: true; } )?`, the empty
/// block refused at its `}`, each key once (at itself).
fn parse_recon_strategy<'s>(c: &mut impl Cursor<'s>) -> R<PReconStrategy> {
    let kind_at = c.pos();
    let kind = c.parse_identifier()?;
    // the engine's grammar names one strategy; the record holds no place for its emitter to refuse at (PARITY row 76)
    if c.dialect().refuses_lite_extensions() && !eq(&kind, "MD5Hash") {
        return Err(crate::cursor::refused_at(
            c.tokens(),
            kind_at,
            jcat!("Unexpected token '", kind, "'"),
        ));
    }
    let mut source_hash = None;
    let mut target_hash = None;
    let mut aggregated = None;
    if c.peek() == T::BRACE_OPEN {
        c.advance()?;
        if c.peek() == T::BRACE_CLOSE {
            return Err(c.error_str("Unexpected token '}'"));
        }
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let k_s = c.pos();
            let k = c.parse_identifier()?;
            once_at(&mut seen, &k, c.tokens(), k_s)?;
            c.expect(T::COLON)?;
            if eq(&k, "sourceHashColumn") {
                source_hash = Some(JStr(c.parse_identifier()?));
            } else if eq(&k, "targetHashColumn") {
                target_hash = Some(JStr(c.parse_identifier()?));
            } else if eq(&k, "aggregatedHash") {
                let b = c.safe_text();
                if !eq(&b, "true") && !eq(&b, "false") {
                    return Err(c.error(jcat!("expected BOOLEAN, got '", b, "'")));
                }
                c.advance()?;
                aggregated = Some(eq(&b, "true"));
            } else {
                return Err(throw_at(c.tokens(), k_s, jcat!("unknown strategy key '", k, "'")));
            }
            c.expect(T::SEMI_COLON)?;
        }
        c.expect(T::BRACE_CLOSE)?;
    }
    Ok(PReconStrategy {
        kind: JStr(kind),
        source_hash_column: source_hash,
        target_hash_column: target_hash,
        aggregated_hash: aggregated,
    })
}
