// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `parser.MappingProtocolParser`, the tests a `###Mapping` carries (docs/rust-port/PHASE2.md §4 step 4):
//! `testSuites` (`parseTestSuite`: a suite's function, its tests, their store data and assertions) and the legacy
//! `MappingTests` (`parseLegacyTest`: query, input data, assert), on the mapping cursor. A suite's function and a
//! legacy query are parsed with the mapping's path as their spans' source id.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, throw_at, unquote_path_iae};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::mapping_parser::{MappingCursor, embedded_span, require_unique_model_type};
use crate::nodes::boxed;
use crate::records::{
    PAssertionValue, PEqualToValue, PLegacyInputData, PLegacyMappingTest, PMappingTest, PMappingTestSuite, PPointer,
    PRelationElement, PStoreTestData, PTestAssertion, SourceInfo, ValueSpecification,
};
use crate::spec_parser::{parse_code_block_tokens, parse_tokens};
use crate::ustr::{eq, u};

use TokenType as T;

fn si(start_line: i32, start_column: i32, end_line: i32, end_column: i32) -> SourceInfo {
    SourceInfo {
        source_id: JStr::default(),
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

fn store_pointer(path: &[u16], span: &SourceInfo) -> Box<PPointer> {
    Box::new(PPointer {
        r#type: JStr::from("STORE"),
        path: JStr(path.to_vec()),
        source_information: boxed(span.clone()),
    })
}

/// Java's `String.replace(target, replacement)` over UTF-16 units, left to right.
pub(crate) fn replace_units(s: &[u16], target: &[u16], replacement: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(target) {
            out.extend_from_slice(replacement);
            i += target.len();
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

impl MappingCursor<'_, '_> {
    /// A top-level-`;`-delimited code block at the cursor (a suite's `function:`, a legacy `query:`), with the
    /// mapping's path as its spans' source id.
    fn mapping_lambda(&mut self, mapping_fqn: &[u16]) -> R<Vec<ValueSpecification>> {
        let f_s = self.pos;
        let mut depth = 0i64;
        while !self.at_end() {
            let t = self.peek();
            if depth == 0 && t == T::SEMI_COLON {
                break;
            }
            if matches!(t, T::PAREN_OPEN | T::BRACKET_OPEN | T::BRACE_OPEN) {
                depth += 1;
            } else if matches!(t, T::PAREN_CLOSE | T::BRACKET_CLOSE | T::BRACE_CLOSE) {
                depth -= 1;
            }
            self.advance()?;
        }
        parse_code_block_tokens(
            &self.tokens.slice(f_s, self.pos),
            JStr(mapping_fqn.to_vec()),
            self.dialect,
        )
    }

    /// `parseTestSuite(mappingFqn)`: `id: { doc: '...'; function: |...; tests: [...]; }`, its span the id through the
    /// close.
    pub(crate) fn parse_test_suite(&mut self, mapping_fqn: &[u16]) -> R<PMappingTestSuite> {
        let s_s = self.pos;
        let suite_id = self.parse_identifier()?;
        self.expect(T::COLON)?;
        self.expect(T::BRACE_OPEN)?;
        let mut func = None;
        let mut tests = Vec::new();
        let mut doc = None;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let key = self.text();
            if eq(key, "doc") {
                if doc.is_some() {
                    return Err(self.error_str("Field 'doc' should be specified only once"));
                }
                self.advance()?;
                self.expect(T::COLON)?;
                doc = Some(JStr(self.consume_string_literal("'doc'")?));
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "function") {
                if func.is_some() {
                    return Err(self.error_str("Field 'function' should be specified only once"));
                }
                self.advance()?;
                self.expect(T::COLON)?;
                let mut body = self.mapping_lambda(mapping_fqn)?;
                if body.len() != 1 {
                    return Err(self.error_str("suite function must be ONE lambda"));
                }
                func = Some(body.remove(0));
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "tests") {
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    tests.push(Box::new(self.parse_mapping_test()?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                self.match_tok(T::SEMI_COLON)?;
                continue;
            }
            return Err(self.error(jcat!("mapping test-suite key '", self.safe_text(), "' is unbuilt")));
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        let Some(func) = func else {
            return Err(throw_at(
                self.tokens,
                s_s,
                JStr::from("mapping test suite without a function"),
            ));
        };
        Ok(PMappingTestSuite {
            id: JStr(suite_id),
            doc,
            func,
            tests,
            source_information: self.span(s_s, close),
        })
    }

    /// `parseLegacyTest(mappingFqn)`: `name ( query: |...; data: [<Kind, FORMAT, path, 'data' + ...>]; assert:
    /// '...'; )` -- the data's parts joined, a relational one unescaped (its `\;` guarded), an object one raw.
    pub(crate) fn parse_legacy_test(&mut self, mapping_fqn: &[u16]) -> R<PLegacyMappingTest> {
        let t_s = self.pos;
        let name = self.parse_identifier()?;
        self.expect(T::PAREN_OPEN)?;
        let mut query = None;
        let mut input_data = Vec::new();
        let mut expected: Option<Vec<u16>> = None;
        let mut assert_span = None;
        let mut seen_data = false;
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let key = self.text();
            if eq(key, "query") || self.peek() == T::MAPPING_TESTS_QUERY {
                if query.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        t_s,
                        JStr::from("Field 'query' should be specified only once"),
                    ));
                }
                self.advance()?;
                self.expect(T::COLON)?;
                let mut body = self.mapping_lambda(mapping_fqn)?;
                if body.len() != 1 {
                    return Err(self.error_str("legacy test query must be ONE lambda"));
                }
                query = Some(body.remove(0));
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "data") {
                if seen_data {
                    return Err(throw_at(
                        self.tokens,
                        t_s,
                        JStr::from("Field 'data' should be specified only once"),
                    ));
                }
                seen_data = true;
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    let d_s = self.pos;
                    self.expect(T::LESS_THAN)?;
                    let kind = self.parse_identifier()?;
                    let relational = eq(&kind, "Relational");
                    if !relational && !eq(&kind, "Object") {
                        return Err(self.error(jcat!("legacy input kind '", kind, "' is unbuilt")));
                    }
                    self.expect(T::COMMA)?;
                    let mut heads: Vec<Vec<u16>> = Vec::new();
                    while !self.at_end() && self.peek() != T::STRING {
                        heads.push(unquote_path_iae(&self.parse_qualified_name()?)?);
                        self.expect(T::COMMA)?;
                    }
                    let kind_word = if relational { "relational" } else { "object" };
                    if heads.len() != 2 {
                        return Err(self.error(jcat!("Mapping test ", kind_word, " 'input type' is missing")));
                    }
                    let input_type = heads.remove(0);
                    let known_format = if relational {
                        eq(&input_type, "SQL") || eq(&input_type, "CSV")
                    } else {
                        eq(&input_type, "JSON") || eq(&input_type, "XML")
                    };
                    if !known_format {
                        return Err(self.error(jcat!(
                            "Mapping test ",
                            kind_word,
                            " input data does not support format '",
                            input_type,
                            "'. Possible values: ",
                            if relational { "SQL, CSV" } else { "JSON, XML" }
                        )));
                    }
                    let target_path = heads.remove(0);
                    let mut data_buf: Vec<u16> = Vec::new();
                    loop {
                        let raw = self.slack_text();
                        if self.peek() != T::STRING {
                            return Err(self.error(jcat!(
                                "expected a quoted string in the test input data, got ",
                                self.peek()
                            )));
                        }
                        if relational {
                            let guarded = replace_units(raw, &u("\\;"), &u("\\\\;"));
                            data_buf.extend(self.unquote_and_unescape(&guarded)?);
                        } else {
                            if raw.len() < 2 {
                                // as `assert`'s below (PARITY row 85)
                                return Err(
                                    throw_at(self.tokens, self.pos, jcat!("Unexpected token '", raw, "'")).as_refusal()
                                );
                            }
                            data_buf.extend_from_slice(&raw[1..raw.len() - 1]);
                        }
                        self.advance()?;
                        if !self.match_tok(T::PLUS)? {
                            break;
                        }
                    }
                    let d_e = self.pos;
                    self.expect(T::GREATER_THAN)?;
                    input_data.push(Box::new(PLegacyInputData {
                        relational,
                        target_path: JStr(target_path),
                        input_type: JStr(input_type),
                        data: JStr(data_buf),
                        source_information: self.span(d_s, d_e),
                    }));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "assert") {
                if expected.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        t_s,
                        JStr::from("Field 'assert' should be specified only once"),
                    ));
                }
                let a_s = self.pos;
                self.advance()?;
                self.expect(T::COLON)?;
                if self.at_end() || self.peek() != T::STRING {
                    return Err(self.error_str("expected a string literal for 'assert'"));
                }
                let raw = self.text();
                if raw.len() < 2 {
                    // a `'` left open by the end of the input: refused at the quote (PARITY row 85)
                    return Err(throw_at(self.tokens, self.pos, jcat!("Unexpected token '", raw, "'")).as_refusal());
                }
                expected = Some(raw[1..raw.len() - 1].to_vec());
                self.advance()?;
                if self.pos >= self.tokens.count() {
                    // the engine's place for the missing `;`: the end of the input (PARITY row 29)
                    return Err(crate::cursor::refused_at_eof(
                        self.tokens,
                        jcat!("expected ", T::SEMI_COLON, " but found EOF ('<EOF>')"),
                    ));
                }
                assert_span = self.span(a_s, self.pos);
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            return Err(self.error(jcat!("legacy mapping-test key '", self.safe_text(), "' is unbuilt")));
        }
        let close = self.pos;
        self.expect(T::PAREN_CLOSE)?;
        if !seen_data {
            return Err(throw_at(self.tokens, t_s, JStr::from("Field 'data' is required")));
        }
        let (Some(query), Some(expected), Some(_)) = (query, expected, &assert_span) else {
            return Err(throw_at(
                self.tokens,
                t_s,
                JStr::from("legacy mapping test needs query AND assert"),
            ));
        };
        Ok(PLegacyMappingTest {
            name: JStr(name),
            query,
            input_data,
            expected_output: JStr(expected),
            assert_source_information: assert_span,
            source_information: self.span(t_s, close),
        })
    }

    /// `parseMappingTest`: `id: { doc: '...'; data: [...]; asserts: [...]; }`, an assertion required.
    fn parse_mapping_test(&mut self) -> R<PMappingTest> {
        let t_s = self.pos;
        let test_id = self.parse_identifier()?;
        self.expect(T::COLON)?;
        self.expect(T::BRACE_OPEN)?;
        let mut data = Vec::new();
        let mut asserts = Vec::new();
        let mut doc = None;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let key = self.text();
            if eq(key, "doc") {
                self.advance()?;
                self.expect(T::COLON)?;
                doc = Some(JStr(self.consume_string_literal("'doc'")?));
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "data") {
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    data.push(Box::new(self.parse_store_test_data()?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            if eq(key, "asserts") {
                self.advance()?;
                self.expect(T::COLON)?;
                self.expect(T::BRACKET_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                    asserts.push(Box::new(self.parse_test_assertion()?));
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::BRACKET_CLOSE)?;
                self.expect(T::SEMI_COLON)?;
                continue;
            }
            return Err(self.error(jcat!("mapping test key '", self.safe_text(), "' is unbuilt")));
        }
        if asserts.is_empty() {
            return Err(throw_at(self.tokens, t_s, JStr::from("Field 'asserts' is required")));
        }
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PMappingTest {
            id: JStr(test_id),
            doc,
            store_test_data: data,
            assertions: asserts,
            source_information: self.span(t_s, close),
        })
    }

    /// `parseStoreTestData`: `Store: Reference #{ path }#`, `Relation #{ ... }#`, `ModelStore #{ ... }#`, or any other
    /// embedded value -- each its own span detail.
    fn parse_store_test_data(&mut self) -> R<PStoreTestData> {
        let s_s = self.pos;
        let store_path = unquote_path_iae(&self.parse_qualified_name()?)?;
        let store_span = self.span_of(s_s, self.pos - 1);
        self.expect(T::COLON)?;
        let ts = self.tokens;
        if self.peek() == T::VALID_STRING && eq(self.text(), "Reference") {
            let ref_tok = self.pos;
            self.advance()?;
            let ref_island = self.read_island()?;
            let mut ri = MappingCursor::at(&ref_island.tokens, 0, self.dialect);
            let data_path = unquote_path_iae(&ri.parse_qualified_name()?)?;
            let de = PPointer {
                r#type: JStr::from("DATA"),
                path: JStr(data_path),
                source_information: boxed(si(
                    ts.start_line(ref_tok) as i32,
                    ts.start_column(ref_tok) as i32,
                    ref_island.end_line,
                    ref_island.end_column,
                )),
            };
            return Ok(PStoreTestData {
                store: store_pointer(&store_path, &store_span),
                model_data: None,
                model_store_source_information: None,
                data_element: Some(Box::new(de)),
                relation_elements: None,
                relation_accessor_source_information: None,
                embedded: None,
                source_information: boxed(si(
                    store_span.start_line,
                    store_span.start_column,
                    ref_island.end_line,
                    ref_island.end_column,
                )),
            });
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "Relation") {
            self.advance()?;
            let ri = self.read_island()?;
            let rels = self.parse_relation_elements(&ri, true)?;
            let first = match rels.first() {
                None => self.span_of(self.pos - 1, self.pos - 1),
                Some(r) => r
                    .source_information
                    .as_deref()
                    .expect("a parsed node always carries its span")
                    .clone(),
            };
            let accessor = si(first.start_line, first.start_column, ri.end_line, ri.end_column + 3);
            return Ok(PStoreTestData {
                store: store_pointer(&store_path, &store_span),
                model_data: None,
                model_store_source_information: None,
                data_element: None,
                relation_elements: Some(rels),
                relation_accessor_source_information: boxed(accessor),
                embedded: None,
                source_information: boxed(si(
                    store_span.start_line,
                    store_span.start_column,
                    ri.end_line,
                    ri.end_column,
                )),
            });
        }
        if !(self.peek() == T::VALID_STRING && eq(self.text(), "ModelStore")) {
            let v = self.parse_embedded_value()?;
            let v_si = embedded_span(&v);
            return Ok(PStoreTestData {
                store: store_pointer(&store_path, &store_span),
                model_data: None,
                model_store_source_information: None,
                data_element: None,
                relation_elements: None,
                relation_accessor_source_information: None,
                embedded: Some(v),
                source_information: boxed(si(
                    store_span.start_line,
                    store_span.start_column,
                    v_si.end_line,
                    v_si.end_column,
                )),
            });
        }
        let ms_tok = self.pos;
        self.advance()?;
        let island = self.read_island()?;
        let mut model_data = Vec::new();
        let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
        while !inner.at_end() {
            let entry = inner.parse_model_entry()?;
            require_unique_model_type(&model_data, &entry)?;
            model_data.push(entry);
            inner.match_tok(T::COMMA)?;
        }
        let ms_span = si(
            ts.start_line(ms_tok) as i32,
            ts.start_column(ms_tok) as i32,
            island.end_line,
            island.end_column,
        );
        Ok(PStoreTestData {
            store: store_pointer(&store_path, &store_span),
            model_data: Some(model_data),
            model_store_source_information: boxed(ms_span),
            data_element: None,
            relation_elements: None,
            relation_accessor_source_information: None,
            embedded: None,
            source_information: boxed(si(
                store_span.start_line,
                store_span.start_column,
                island.end_line,
                island.end_column,
            )),
        })
    }

    /// `parseTestAssertion`: `id: <assertion value>`.
    pub(crate) fn parse_test_assertion(&mut self) -> R<PTestAssertion> {
        let assert_id = self.parse_identifier()?;
        self.expect(T::COLON)?;
        self.assertion_value(assert_id)
    }

    /// `assertionValue(assertId)`: `ExternalFormat #{...}#` (the compact `equalToJson`), `Relation #{...}#`, `EqualTo
    /// #{ expected: <value>; }#` or `EqualToJson #{ expected: ExternalFormat #{...}#; }#`.
    pub(crate) fn assertion_value(&mut self, assert_id: Vec<u16>) -> R<PTestAssertion> {
        let ts = self.tokens;
        if self.peek() == T::VALID_STRING && eq(self.text(), "ExternalFormat") {
            let ef = self.parse_external_format()?;
            self.match_tok(T::SEMI_COLON)?;
            let span = ef.source_information.clone();
            return Ok(PTestAssertion {
                id: JStr(assert_id),
                expected: PAssertionValue::PExternalFormatData(Box::new(ef)),
                source_information: span,
            });
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "Relation") {
            let r_tok = self.pos;
            self.advance()?;
            let brace_tok = self.pos;
            let ri = self.read_island()?;
            let mut rels = self.parse_relation_elements(&ri, false)?;
            if rels.len() != 1 {
                return Err(self.error_str("Relation assertion must be ONE element"));
            }
            self.match_tok(T::SEMI_COLON)?;
            let re = rels.remove(0);
            let es = re
                .source_information
                .as_deref()
                .expect("a parsed node always carries its span")
                .clone();
            let shifted = PRelationElement {
                columns: re.columns,
                paths: re.paths,
                rows: re.rows,
                source_information: boxed(si(
                    ts.start_line(brace_tok) as i32,
                    ts.start_column(brace_tok) as i32 + 2,
                    es.end_line - 1,
                    es.end_column,
                )),
            };
            return Ok(PTestAssertion {
                id: JStr(assert_id),
                expected: PAssertionValue::PRelationElement(Box::new(shifted)),
                source_information: boxed(si(
                    ts.start_line(r_tok) as i32,
                    ts.start_column(r_tok) as i32,
                    ri.end_line,
                    ri.end_column,
                )),
            });
        }
        if self.peek() == T::VALID_STRING && eq(self.text(), "EqualTo") {
            let eq_tok = self.pos;
            self.advance()?;
            let island = self.read_island()?;
            let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
            if !eq(inner.slack_text(), "expected") {
                return Err(inner.error(jcat!("assertion key '", inner.safe_text(), "' is unbuilt")));
            }
            inner.advance()?;
            inner.expect(T::COLON)?;
            let v_s = inner.pos;
            let mut d = 0i64;
            while !inner.at_end() {
                let tk = inner.peek();
                if d == 0 && tk == T::SEMI_COLON {
                    break;
                }
                if matches!(tk, T::PAREN_OPEN | T::BRACE_OPEN | T::BRACKET_OPEN) {
                    d += 1;
                } else if matches!(tk, T::PAREN_CLOSE | T::BRACE_CLOSE | T::BRACKET_CLOSE) {
                    d -= 1;
                }
                inner.advance()?;
            }
            let value = parse_tokens(&inner.tokens.slice(v_s, inner.pos), self.dialect)?;
            return Ok(PTestAssertion {
                id: JStr(assert_id),
                expected: PAssertionValue::PEqualToValue(Box::new(PEqualToValue { value })),
                source_information: boxed(si(
                    ts.start_line(eq_tok) as i32,
                    ts.start_column(eq_tok) as i32,
                    island.end_line,
                    island.end_column,
                )),
            });
        }
        if !(self.peek() == T::VALID_STRING && eq(self.text(), "EqualToJson")) {
            return Err(self.error(jcat!("test assertion kind '", self.safe_text(), "' is unbuilt")));
        }
        let eq_tok = self.pos;
        self.advance()?;
        let island = self.read_island()?;
        let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
        if !eq(inner.slack_text(), "expected") {
            return Err(inner.error(jcat!("assertion key '", inner.safe_text(), "' is unbuilt")));
        }
        inner.advance()?;
        inner.expect(T::COLON)?;
        if !(inner.peek() == T::VALID_STRING && eq(inner.text(), "ExternalFormat")) {
            return Err(inner.error(jcat!("assertion payload '", inner.safe_text(), "' is unbuilt")));
        }
        let ef = inner.parse_external_format()?;
        inner.match_tok(T::SEMI_COLON)?;
        Ok(PTestAssertion {
            id: JStr(assert_id),
            expected: PAssertionValue::PExternalFormatData(Box::new(ef)),
            source_information: boxed(si(
                ts.start_line(eq_tok) as i32,
                ts.start_column(eq_tok) as i32,
                island.end_line,
                island.end_column,
            )),
        })
    }
}
