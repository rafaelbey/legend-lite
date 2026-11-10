// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `parser.MappingProtocolParser`, the part a `###Data` element reaches (docs/rust-port/PHASE2.md §4 step 4):
//! `parseData` and the one embedded-data value grammar (`ExternalFormat`, `Reference`, `DataspaceTestData`,
//! `Relation`, `Relational`, `ModelStore`, `ServiceStore` and its stubs, `ServiceStubDataParser`). The `###Mapping`
//! grammar is on this same cursor ([`MappingCursor`], Java's `MappingProtocolParser`): its elements in
//! `mapping_grammar`, its tests in `mapping_tests`.
//!
//! An island's content (`#{ ... }#`) is lexed again in place, in normal mode (`readIsland`, the lexer's `lex_range`),
//! and read by a cursor of its own; its offsets and lines stay the document's.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, is_fqn_segment_token, throw_at, unquote_path_iae};
use crate::doors::{Dialect, JavaError};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::{TokenStream, TokenType};
use crate::nodes::boxed;
use crate::records::{
    PDataBody, PDataElement, PDataReference, PDataResolver, PElementRef, PEmbeddedDataValue, PExternalFormatData,
    PModelData, PModelEmbeddedData, PModelInstanceData, PModelStoreData, PPointer, PRelationData, PRelationElement,
    PRelationalCsvData, PRelationalCsvTable, PServiceStoreData, PServiceStub, PStringValuePattern, PStubParam,
    PureCollection, SourceInfo, ValueSpecification,
};
use crate::spec_parser::parse_instance_data;
use crate::ustr::{ends_with, eq, last_index_of, last_index_of_str, strip, u};
use lite_javacompat::character::is_whitespace;
use std::collections::HashSet;

use TokenType as T;

/// Java's `MappingProtocolParser` as a cursor: a token stream, a position, the dialect it serves, the mapping
/// section's first content line (-1 unknown: an AggregationAware member then refuses) and the lines the current
/// `~modelOperation` block's lambdas shift up. Its spans carry no source id.
pub struct MappingCursor<'t, 's> {
    pub(crate) tokens: &'t TokenStream<'s>,
    pub(crate) pos: usize,
    pub(crate) dialect: Dialect,
    pub(crate) section_start_line: i64,
    pub(crate) agg_lambda_shift: i64,
}

impl<'s> Cursor<'s> for MappingCursor<'_, 's> {
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

/// An island's content lexed in place (`MappingProtocolParser.IslandBlock`): its tokens, the closing `}#`'s end line
/// and column, the content's offsets in the document.
pub struct IslandBlock<'s> {
    pub tokens: TokenStream<'s>,
    pub end_line: i32,
    pub end_column: i32,
    pub content_start: usize,
    pub content_end: usize,
}

/// `MappingProtocolParser.parseData(ts, tokenIndex, dialect, endOut)`: one `Data` element at `token_index`, and the
/// token where it ended.
pub fn parse_data(ts: &TokenStream<'_>, token_index: usize, dialect: Dialect) -> R<(PDataElement, usize)> {
    let mut p = MappingCursor::at(ts, token_index, dialect);
    let d = p.parse_data_element()?;
    Ok((d, p.pos))
}

pub(crate) fn span_info(line: usize, column: usize, end_line: i32, end_column: i32) -> SourceInfo {
    SourceInfo {
        source_id: JStr::default(),
        start_line: line as i32,
        start_column: column as i32,
        end_line,
        end_column,
    }
}

impl<'t, 's> MappingCursor<'t, 's> {
    /// `new MappingProtocolParser(tokens, pos, dialect)`: no section start line, no lambda shift.
    pub fn at(tokens: &'t TokenStream<'s>, pos: usize, dialect: Dialect) -> MappingCursor<'t, 's> {
        MappingCursor {
            tokens,
            pos,
            dialect,
            section_start_line: -1,
            agg_lambda_shift: 0,
        }
    }

    /// `expectText(want)`: the token's text is `want`, else `expected 'want' but found '...'`.
    pub fn expect_text(&mut self, want: &str) -> R<()> {
        if !eq(self.slack_text(), want) {
            return Err(self.error(jcat!("expected '", want, "' but found '", self.safe_text(), "'")));
        }
        self.advance()
    }

    /// `parseDataElement`: `Data <<st>> {tags} qn { value? resolvers* }`, its documentation first.
    fn parse_data_element(&mut self) -> R<PDataElement> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        self.expect_text("Data")?;
        let dec = self.parse_decorations()?;
        let tagged_values = self.tagged_values_with_documentation(doc, dec.tagged_values)?;
        let qn = unquote_path_iae(&self.parse_qualified_name()?)?;
        let (pkg, name) = match last_index_of_str(&qn, "::") {
            None => (Vec::new(), qn.clone()),
            Some(cut) => (qn[..cut].to_vec(), qn[cut + 2..].to_vec()),
        };
        self.expect(T::BRACE_OPEN)?;
        let mut value = None;
        let mut resolvers = Vec::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            match self.fqn_terminator() {
                1 => resolvers.push(Box::new(self.parse_data_resolver()?)),
                2 => {
                    let r_s = self.pos;
                    let path = unquote_path_iae(&self.parse_qualified_name()?)?;
                    let r_span = self.span(r_s, self.pos - 1);
                    self.expect(T::SEMI_COLON)?;
                    resolvers.push(Box::new(PDataResolver {
                        element_pointer: Box::new(PElementRef {
                            path: JStr(path),
                            source_information: r_span.clone(),
                        }),
                        data: None,
                        source_information: r_span,
                    }));
                }
                _ => {
                    if value.is_some() {
                        return Err(self.error_str("a Data element carries ONE embedded value"));
                    }
                    value = Some(self.parse_embedded_value()?);
                }
            }
        }
        let body = PDataBody { value, resolvers };
        let close = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        Ok(PDataElement {
            pkg: JStr(pkg),
            name: JStr(name),
            body: Box::new(body),
            stereotypes: dec.stereotypes,
            tagged_values,
            source_information: self.span(decl_start, close),
        })
    }

    /// `fqnTerminator`: 1 for a qualified name then `:` (a base resolver), 2 for one then `;` (a reference), 0 else.
    fn fqn_terminator(&self) -> i32 {
        let ts = self.tokens;
        let mut p = self.pos;
        while p < ts.count() && (is_fqn_segment_token(ts.ty(p)) || ts.ty(p) == T::PATH_SEPARATOR) {
            p += 1;
        }
        if p == self.pos || p >= ts.count() {
            return 0;
        }
        match ts.ty(p) {
            T::COLON => 1,
            T::SEMI_COLON => 2,
            _ => 0,
        }
    }

    /// `parseDataResolver`: `store::S: <value>;`, its span the store path through the value's (raw) close.
    fn parse_data_resolver(&mut self) -> R<PDataResolver> {
        let p_s = self.pos;
        let store = unquote_path_iae(&self.parse_qualified_name()?)?;
        let store_span = self.span_of(p_s, self.pos - 1);
        self.expect(T::COLON)?;
        let value = self.parse_embedded_value()?;
        let v_span = embedded_span(&value);
        self.expect(T::SEMI_COLON)?;
        Ok(PDataResolver {
            element_pointer: Box::new(PElementRef {
                path: JStr(store),
                source_information: boxed(store_span.clone()),
            }),
            data: Some(value),
            source_information: boxed(SourceInfo {
                source_id: JStr::default(),
                start_line: store_span.start_line,
                start_column: store_span.start_column,
                end_line: v_span.end_line,
                end_column: v_span.end_column,
            }),
        })
    }

    /// `parseEmbeddedValue`: one embedded-data value -- `Relational`, `ExternalFormat`, `Reference` /
    /// `DataspaceTestData`, `Relation`, `ModelStore` or `ServiceStore`, each `Kind #{...}#`.
    pub fn parse_embedded_value(&mut self) -> R<PEmbeddedDataValue> {
        if self.peek() == T::RELATIONAL {
            return Ok(PEmbeddedDataValue::PRelationalCsvData(Box::new(
                self.parse_relational_csv()?,
            )));
        }
        let kind = self.slack_text();
        if eq(kind, "ExternalFormat") {
            return Ok(PEmbeddedDataValue::PExternalFormatData(Box::new(
                self.parse_external_format()?,
            )));
        }
        if eq(kind, "Reference") || eq(kind, "DataspaceTestData") {
            let r_tok = self.pos;
            self.advance()?;
            let ri = self.read_island()?;
            let mut rp = MappingCursor::at(&ri.tokens, 0, self.dialect);
            let data_path = unquote_path_iae(&rp.parse_qualified_name()?)?;
            // the engine takes the island's text whole for a path: what is left over makes it no identifier (ruling 18)
            if self.dialect.refuses_lite_extensions() && !rp.at_end() {
                let words = crate::jcat!("Invalid identifier: ", ri.tokens.text(rp.pos()));
                return Err(crate::cursor::throw_at(self.tokens, r_tok, words).as_refusal());
            }
            let si = span_info(
                self.tokens.start_line(r_tok),
                self.tokens.start_column(r_tok),
                ri.end_line,
                ri.end_column,
            );
            return Ok(PEmbeddedDataValue::PDataReference(Box::new(PDataReference {
                data_element: Box::new(PPointer {
                    r#type: JStr::from(if eq(kind, "Reference") { "DATA" } else { "DATASPACE" }),
                    path: JStr(data_path),
                    source_information: boxed(si.clone()),
                }),
                source_information: boxed(si),
            })));
        }
        if eq(kind, "Relation") {
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
            return Ok(PEmbeddedDataValue::PRelationData(Box::new(PRelationData {
                relation_elements: rels,
                source_information: boxed(SourceInfo {
                    source_id: JStr::default(),
                    start_line: first.start_line,
                    start_column: first.start_column,
                    end_line: ri.end_line,
                    end_column: ri.end_column + 3,
                }),
            })));
        }
        if eq(kind, "ModelStore") {
            let ms_tok = self.pos;
            self.advance()?;
            let island = self.read_island()?;
            let mut model_data: Vec<PModelData> = Vec::new();
            let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
            while !inner.at_end() {
                let entry = inner.parse_model_entry()?;
                require_unique_model_type(&model_data, &entry)?;
                model_data.push(entry);
                inner.list_comma()?;
            }
            return Ok(PEmbeddedDataValue::PModelStoreData(Box::new(PModelStoreData {
                model_data,
                source_information: boxed(span_info(
                    self.tokens.start_line(ms_tok),
                    self.tokens.start_column(ms_tok),
                    island.end_line,
                    island.end_column,
                )),
            })));
        }
        if eq(kind, "ServiceStore") {
            let k_tok = self.pos;
            self.advance()?;
            let si = self.read_island()?;
            let mut inner = MappingCursor::at(&si.tokens, 0, self.dialect);
            let mut stubs = Vec::new();
            inner.expect(T::BRACKET_OPEN)?;
            while inner.peek() != T::BRACKET_CLOSE && !inner.at_end() {
                stubs.push(Box::new(inner.parse_service_stub()?));
                inner.list_comma()?;
            }
            inner.expect(T::BRACKET_CLOSE)?;
            // `serviceStubMappings EOF`, at the engine's dialect (the user's ruling 20 of 2026-11-10,
            // docs/rust-port/PHASE2.md section 6; register S188)
            if self.dialect.holds_to_the_engines_walk() && !inner.at_end() {
                return Err(inner.unexpected());
            }
            return Ok(PEmbeddedDataValue::PServiceStoreData(Box::new(PServiceStoreData {
                stubs,
                source_information: boxed(span_info(
                    self.tokens.start_line(k_tok),
                    self.tokens.start_column(k_tok),
                    si.end_line,
                    si.end_column,
                )),
            })));
        }
        Err(self.error(jcat!("embedded data kind '", self.safe_text(), "' is unbuilt")))
    }

    /// `parseRelationalCsv`: `Relational #{ schema.table: 'csv' + 'csv'; ... }#`, each table's span its schema token
    /// through its `;`.
    fn parse_relational_csv(&mut self) -> R<PRelationalCsvData> {
        let kw_tok = self.pos;
        self.expect(T::RELATIONAL)?;
        let island = self.read_island()?;
        let mut tables = Vec::new();
        let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
        while !inner.at_end() {
            let t_s = inner.pos;
            let schema = unquote_path_iae(&inner.parse_identifier()?)?;
            inner.expect(T::DOT)?;
            let table = unquote_path_iae(&inner.parse_identifier()?)?;
            inner.expect(T::COLON)?;
            let mut values: Vec<u16> = Vec::new();
            loop {
                let quoted = inner.slack_text();
                inner.expect(T::STRING)?;
                values.extend(inner.unquote_and_unescape(quoted)?);
                if !inner.match_tok(T::PLUS)? {
                    break;
                }
            }
            let semi = inner.pos;
            inner.expect(T::SEMI_COLON)?;
            tables.push(Box::new(PRelationalCsvTable {
                schema: JStr(schema),
                table: JStr(table),
                values: Some(JStr(values)),
                source_information: inner.span(t_s, semi),
            }));
        }
        Ok(PRelationalCsvData {
            tables,
            source_information: boxed(span_info(
                self.tokens.start_line(kw_tok),
                self.tokens.start_column(kw_tok),
                island.end_line,
                island.end_column,
            )),
        })
    }

    /// `parseModelEntry`: one `path: <value>` of a ModelStore island -- an embedded value, or a `[ ^X(...) ]`
    /// instance collection read as legend-engine's model-data walker reads it (its outer collection unspanned).
    pub(crate) fn parse_model_entry(&mut self) -> R<PModelData> {
        let m_s = self.pos;
        let model = unquote_path_iae(&self.parse_qualified_name()?)?;
        self.expect(T::COLON)?;
        let ts = self.tokens;
        if self.peek() == T::BRACKET_OPEN {
            let l_s = self.pos;
            self.skip_bracket_block()?;
            let close = self.pos - 1;
            let mut vs = parse_instance_data(&ts.slice(l_s, self.pos), self.span_source_id(), self.dialect)?;
            if vs.len() != 1 {
                return Err(self.error_str("model instance data must be ONE collection"));
            }
            let instances = match vs.remove(0) {
                ValueSpecification::PureCollection(c) => ValueSpecification::PureCollection(Box::new(PureCollection {
                    values: c.values,
                    pos: None,
                })),
                v => v,
            };
            return Ok(PModelData::PModelInstanceData(Box::new(PModelInstanceData {
                model: JStr(model),
                instances,
                source_information: boxed(span_info(
                    ts.start_line(m_s),
                    ts.start_column(m_s),
                    ts.end_line(close) as i32,
                    ts.end_column(close) as i32,
                )),
            })));
        }
        let val = self.parse_embedded_value()?;
        let v_si = embedded_span(&val);
        Ok(PModelData::PModelEmbeddedData(Box::new(PModelEmbeddedData {
            model: JStr(model),
            data: val,
            source_information: boxed(span_info(
                ts.start_line(m_s),
                ts.start_column(m_s),
                v_si.end_line,
                v_si.end_column,
            )),
        })))
    }

    /// `skipBracketBlock`: a bracket-balanced block from its `[`.
    fn skip_bracket_block(&mut self) -> R<()> {
        self.expect(T::BRACKET_OPEN)?;
        let mut depth = 0i64;
        while !self.at_end() {
            match self.peek() {
                T::BRACKET_OPEN | T::PAREN_OPEN | T::BRACE_OPEN => depth += 1,
                T::PAREN_CLOSE | T::BRACE_CLOSE => depth -= 1,
                T::BRACKET_CLOSE => {
                    if depth == 0 {
                        self.advance()?;
                        return Ok(());
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.advance()?;
        }
        Err(self.error_str("unterminated instance collection"))
    }

    /// `parseExternalFormat`: `ExternalFormat #{ contentType: '...'; data: '...'; }#`, its span the keyword through
    /// the `}#`.
    pub(crate) fn parse_external_format(&mut self) -> R<PExternalFormatData> {
        let ef_tok = self.pos;
        self.advance()?; // ExternalFormat
        let island = self.read_island()?;
        let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
        let mut content_type = None;
        let mut data = None;
        while !inner.at_end() {
            let key = inner.text();
            inner.advance()?;
            inner.expect(T::COLON)?;
            let v = inner.unquote_and_unescape(inner.slack_text())?;
            inner.advance()?;
            inner.expect(T::SEMI_COLON)?;
            if eq(key, "contentType") {
                content_type = Some(v);
            } else if eq(key, "data") {
                data = Some(v);
            } else {
                return Err(inner.error(jcat!("external-format key '", key, "' is unbuilt")));
            }
        }
        let (Some(content_type), Some(data)) = (content_type, data) else {
            return Err(throw_at(
                self.tokens,
                ef_tok,
                JStr::from("external format needs contentType AND data"),
            ));
        };
        Ok(PExternalFormatData {
            content_type: JStr(content_type),
            data: JStr(data),
            source_information: boxed(span_info(
                self.tokens.start_line(ef_tok),
                self.tokens.start_column(ef_tok),
                island.end_line,
                island.end_column,
            )),
        })
    }

    /// `readIsland`: past `#{` to its own `}#` (nested pairs balanced), the content lexed in place.
    pub fn read_island(&mut self) -> R<IslandBlock<'s>> {
        self.expect(T::ISLAND_OPEN)?;
        let ts = self.tokens;
        let slack_start = |i: usize| if i < ts.count() { ts.start(i) } else { 0 };
        let content_start = slack_start(self.pos);
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
            self.advance()?;
        }
        let end_tok = self.pos;
        let content_end = slack_start(end_tok);
        self.expect(T::ISLAND_END)?;
        // past `expect(ISLAND_END)` both offsets are token starts of this stream, in order
        let tokens = ts
            .lex_range(content_start, content_end)
            .expect("an island's content lies inside its source");
        Ok(IslandBlock {
            tokens,
            end_line: ts.end_line(end_tok) as i32,
            end_column: ts.end_column(end_tok) as i32,
            content_start,
            content_end,
        })
    }

    /// `parseRelationElements(island, withPaths)`: a Relation island's `[path:]` / header / rows `;` groups, read from
    /// the document's characters; cells strip both sides.
    pub fn parse_relation_elements(&self, island: &IslandBlock<'_>, with_paths: bool) -> R<Vec<Box<PRelationElement>>> {
        let ts = self.tokens;
        let src = ts.source();
        let mut p = island.content_start;
        let end = island.content_end;
        if with_paths
            && self.dialect.refuses_lite_extensions()
            && let Some(at) = relation_elements_stop(src, p, end)
        {
            return Err(JavaError::parse(
                JStr::from("Unexpected token"),
                ts.line_of(at) as i32,
                ts.column_of(at) as i32,
            )
            .as_refusal());
        }
        let mut out = Vec::new();
        loop {
            while p < end && is_whitespace(src[p]) {
                p += 1;
            }
            if p >= end {
                break;
            }
            let elem_start = p;
            let mut paths = Vec::new();
            if with_paths {
                let eol = line_end(src, p, end);
                let line = strip(&src[p..eol]);
                if !ends_with(line, ":") {
                    return Err(self.error_str("relation data path line must end ':'"));
                }
                split_on(&line[..line.len() - 1], u16::from(b'.'), &mut paths);
                p = eol + 1;
                while p < end && is_whitespace(src[p]) {
                    p += 1;
                }
            }
            let eol = line_end(src, p, end);
            let mut columns = Vec::new();
            let mut col_line = strip(&src[p..eol]);
            let mut rows = Vec::new();
            let mut semi_pos: Option<usize> = None;
            if ends_with(col_line, ";") {
                semi_pos = Some(p + last_index_of(&src[p..eol], u16::from(b';')).expect("the line ends ';'"));
                col_line = &col_line[..col_line.len() - 1];
            }
            split_cells(col_line, &mut columns);
            p = eol + 1;
            while semi_pos.is_none() && p < end {
                let r_eol = line_end(src, p, end);
                let mut line = strip(&src[p..r_eol]);
                if line.is_empty() {
                    p = r_eol + 1;
                    continue;
                }
                let last = ends_with(line, ";");
                if last {
                    semi_pos = Some(p + last_index_of(&src[p..r_eol], u16::from(b';')).expect("the line ends ';'"));
                    line = &line[..line.len() - 1];
                }
                let mut row = Vec::new();
                split_cells(line, &mut row);
                rows.push(row);
                p = r_eol + 1;
                if last {
                    break;
                }
            }
            let Some(semi_pos) = semi_pos else {
                return Err(self.error_str("relation data rows must end with ';'"));
            };
            out.push(Box::new(PRelationElement {
                columns,
                paths,
                rows,
                source_information: boxed(span_info(
                    ts.line_of(elem_start),
                    ts.column_of(elem_start),
                    ts.line_of(semi_pos) as i32,
                    ts.column_of(semi_pos) as i32,
                )),
            }));
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- ServiceStore stubs (ServiceStubDataParser)

    /// `ServiceStubDataParser.parseServiceStub`: `{ request: {...}; response: { body: ...; }; }`, validated in the
    /// engine's order.
    fn parse_service_stub(&mut self) -> R<PServiceStub> {
        let s_tok = self.pos;
        self.expect(T::BRACE_OPEN)?;
        let mut method: Option<Vec<u16>> = None;
        let mut method_spelled = false;
        let mut url = None;
        let mut url_path = None;
        let mut query_params = None;
        let mut header_params = None;
        let mut body_patterns = None;
        let mut request_span = None;
        let mut response_span = None;
        let mut request_key_tok = usize::MAX;
        let mut response_key_tok = usize::MAX;
        let mut body_value: Option<PEmbeddedDataValue> = None;
        let mut body_kind_tok = usize::MAX;
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let key_tok = self.pos;
            let key = self.parse_identifier()?;
            self.expect(T::COLON)?;
            if eq(&key, "request") {
                if request_span.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        s_tok,
                        JStr::from("Field 'request' should be specified only once"),
                    ));
                }
                request_key_tok = key_tok;
                self.expect(T::BRACE_OPEN)?;
                let mut seen_req: HashSet<Vec<u16>> = HashSet::new();
                while !self.at_end() && self.peek() != T::BRACE_CLOSE {
                    let rk = self.parse_identifier()?;
                    if !seen_req.insert(rk.clone()) {
                        return Err(throw_at(
                            self.tokens,
                            request_key_tok,
                            jcat!("Field '", rk, "' should be specified only once"),
                        ));
                    }
                    self.expect(T::COLON)?;
                    if eq(&rk, "method") {
                        method_spelled = true;
                        let m = self.parse_identifier()?;
                        if !(eq(&m, "GET") || eq(&m, "POST")) {
                            return Err(self.error(jcat!(
                                "Unsupported HTTP Method type - ",
                                m,
                                ". Supported types are - GET,POST"
                            )));
                        }
                        method = Some(m);
                    } else if eq(&rk, "url") {
                        url = Some(JStr(self.consume_string_literal("'url'")?));
                    } else if eq(&rk, "urlPath") {
                        url_path = Some(JStr(self.consume_string_literal("'urlPath'")?));
                    } else if eq(&rk, "queryParameters") {
                        query_params = Some(self.parse_stub_params("Query Param")?);
                    } else if eq(&rk, "headerParameters") {
                        header_params = Some(self.parse_stub_params("Header Param")?);
                    } else if eq(&rk, "bodyPatterns") {
                        let mut patterns = Vec::new();
                        self.expect(T::BRACKET_OPEN)?;
                        while !self.at_end() && self.peek() != T::BRACKET_CLOSE {
                            patterns.push(Box::new(self.parse_content_pattern()?));
                        }
                        self.expect(T::BRACKET_CLOSE)?;
                        body_patterns = Some(patterns);
                    } else {
                        return Err(self.error(jcat!("unknown service-stub request key: ", rk)));
                    }
                    self.expect(T::SEMI_COLON)?;
                }
                self.expect(T::BRACE_CLOSE)?;
                self.expect(T::SEMI_COLON)?;
                request_span = self.span(key_tok, self.pos - 1);
            } else if eq(&key, "response") {
                if response_span.is_some() {
                    return Err(throw_at(
                        self.tokens,
                        s_tok,
                        JStr::from("Field 'response' should be specified only once"),
                    ));
                }
                response_key_tok = key_tok;
                self.expect(T::BRACE_OPEN)?;
                while !self.at_end() && self.peek() != T::BRACE_CLOSE {
                    let rk = self.parse_identifier()?;
                    self.expect(T::COLON)?;
                    if !eq(&rk, "body") {
                        return Err(self.error(jcat!("unknown service-stub response key: ", rk)));
                    }
                    if body_value.is_some() {
                        return Err(throw_at(
                            self.tokens,
                            response_key_tok,
                            JStr::from("Field 'body' should be specified only once"),
                        ));
                    }
                    body_kind_tok = self.pos;
                    body_value = Some(self.parse_embedded_value()?);
                    self.expect(T::SEMI_COLON)?;
                }
                self.expect(T::BRACE_CLOSE)?;
                self.expect(T::SEMI_COLON)?;
                response_span = self.span(key_tok, self.pos - 1);
            } else {
                return Err(self.error(jcat!("unknown service-stub key: ", key)));
            }
        }
        let close_tok = self.pos;
        self.expect(T::BRACE_CLOSE)?;
        if request_span.is_none() {
            return Err(throw_at(self.tokens, s_tok, JStr::from("Field 'request' is required")));
        }
        if response_span.is_none() {
            return Err(throw_at(self.tokens, s_tok, JStr::from("Field 'response' is required")));
        }
        if !method_spelled {
            return Err(throw_at(
                self.tokens,
                request_key_tok,
                JStr::from("Field 'method' is required"),
            ));
        }
        let Some(body_value) = body_value else {
            return Err(throw_at(
                self.tokens,
                response_key_tok,
                JStr::from("Field 'body' is required"),
            ));
        };
        let PEmbeddedDataValue::PExternalFormatData(body) = body_value else {
            return Err(throw_at(
                self.tokens,
                body_kind_tok,
                JStr::from("Service response body should be ExternalFormat Embedded Data"),
            ));
        };
        let by_name = |params: Option<Vec<Box<PStubParam>>>| {
            params.map(|mut ps| {
                ps.sort_by(|a, b| a.name.0.cmp(&b.name.0));
                ps
            })
        };
        Ok(PServiceStub {
            method: JStr(method.expect("spelled above")),
            url,
            url_path,
            query_params: by_name(query_params),
            header_params: by_name(header_params),
            body_patterns,
            request_span,
            body,
            response_span,
            source_information: self.span(s_tok, close_tok),
        })
    }

    /// `parseStubParams`: `{ name: <pattern>, 'quoted': <pattern> }`, a repeated name refused at its entry.
    fn parse_stub_params(&mut self, flavor: &str) -> R<Vec<Box<PStubParam>>> {
        self.expect(T::BRACE_OPEN)?;
        let mut out = Vec::new();
        let mut names: HashSet<Vec<u16>> = HashSet::new();
        while !self.at_end() && self.peek() != T::BRACE_CLOSE {
            let p_tok = self.pos;
            let pname = if self.peek() == T::QUOTED_STRING || self.peek() == T::STRING {
                self.consume_string_literal("parameter name")?
            } else {
                self.parse_identifier()?
            };
            self.expect(T::COLON)?;
            let pattern = self.parse_content_pattern()?;
            if !names.insert(pname.clone()) {
                return Err(throw_at(
                    self.tokens,
                    p_tok,
                    jcat!(flavor, " : '", pname, "' value should be defined only once"),
                ));
            }
            out.push(Box::new(PStubParam {
                name: JStr(pname),
                pattern: Box::new(pattern),
            }));
            if !self.match_list_comma()? {
                break;
            }
        }
        self.expect(T::BRACE_CLOSE)?;
        Ok(out)
    }

    /// `parseContentPattern`: `EqualTo #{ expected: '...'; }#` or `EqualToJson #{...}#`.
    fn parse_content_pattern(&mut self) -> R<PStringValuePattern> {
        let kind_tok = self.pos;
        let kind = self.parse_identifier()?;
        let r#type = if eq(&kind, "EqualTo") {
            "equalTo"
        } else if eq(&kind, "EqualToJson") {
            "equalToJson"
        } else {
            return Err(throw_at(
                self.tokens,
                kind_tok,
                jcat!("Unknown contentPattern pattern type: ", kind),
            ));
        };
        let island = self.read_island()?;
        let mut inner = MappingCursor::at(&island.tokens, 0, self.dialect);
        if !eq(inner.slack_text(), "expected") {
            return Err(inner.error(jcat!("unknown contentPattern key: ", inner.safe_text())));
        }
        inner.advance()?;
        inner.expect(T::COLON)?;
        let expected = inner.consume_string_literal("'expected'")?;
        if self.dialect.holds_to_the_engines_walk() {
            // `EXPECTED COLON STRING SEMI_COLON EOF`, at the engine's dialect (the user's ruling 20 of 2026-11-10,
            // docs/rust-port/PHASE2.md section 6; register S188): a `;` the island's end cuts off is `ParserErrorListener`'s
            // empty span there, one column less
            if inner.at_end() {
                return Err(JavaError::parse(
                    JStr::from("Unexpected token '<EOF>'"),
                    self.tokens.line_of(island.content_end) as i32,
                    self.tokens.column_of(island.content_end) as i32 - 1,
                )
                .as_refusal());
            }
            inner.expect(T::SEMI_COLON)?;
            if !inner.at_end() {
                return Err(inner.unexpected());
            }
        } else {
            inner.match_tok(T::SEMI_COLON)?;
        }
        Ok(PStringValuePattern {
            r#type: JStr::from(r#type),
            expected_value: JStr(expected),
        })
    }
}

/// `requireUniqueModelType`: one ModelStore entry per type, a duplicate refused at its own span.
pub(crate) fn require_unique_model_type(seen: &[PModelData], entry: &PModelData) -> R<()> {
    let (model, span) = match entry {
        PModelData::PModelEmbeddedData(d) => (&d.model, &d.source_information),
        PModelData::PModelInstanceData(d) => (&d.model, &d.source_information),
    };
    for d in seen {
        let m = match d {
            PModelData::PModelEmbeddedData(x) => &x.model,
            PModelData::PModelInstanceData(x) => &x.model,
        };
        if m == model {
            let s = span.as_deref().expect("a parsed node always carries its span");
            return Err(JavaError::parse(
                jcat!("Multiple entries found for type: '", model, "'"),
                s.start_line,
                s.start_column,
            ));
        }
    }
    Ok(())
}

/// `embeddedSpan`: the span an embedded value gives its entry -- a relation accessor's own end less the walker's 3.
pub(crate) fn embedded_span(v: &PEmbeddedDataValue) -> SourceInfo {
    let s = match v {
        PEmbeddedDataValue::PExternalFormatData(e) => &e.source_information,
        PEmbeddedDataValue::PDataReference(r) => &r.source_information,
        PEmbeddedDataValue::PModelStoreData(m) => &m.source_information,
        PEmbeddedDataValue::PRelationalCsvData(c) => &c.source_information,
        PEmbeddedDataValue::PServiceStoreData(ss) => &ss.source_information,
        PEmbeddedDataValue::PRelationData(d) => {
            let s = d
                .source_information
                .as_deref()
                .expect("a parsed node always carries its span");
            return SourceInfo {
                end_column: s.end_column - 3,
                source_id: JStr::default(),
                ..s.clone()
            };
        }
    };
    s.as_deref().expect("a parsed node always carries its span").clone()
}

fn line_end(src: &[u16], from: usize, end: usize) -> usize {
    let mut i = from;
    while i < end && src[i] != u16::from(b'\n') {
        i += 1;
    }
    i
}

/// `splitCells`: one CSV line into cells, a `"` grouping commas (and kept), each cell stripped.
fn split_cells(s: &[u16], out: &mut Vec<JStr>) {
    let mut start = 0;
    let mut quoted = false;
    for i in 0..=s.len() {
        if i < s.len() && s[i] == u16::from(b'"') {
            quoted = !quoted;
        } else if i == s.len() || (s[i] == u16::from(b',') && !quoted) {
            out.push(JStr(strip(&s[start..i]).to_vec()));
            start = i + 1;
        }
    }
}

fn split_on(s: &[u16], sep: u16, out: &mut Vec<JStr>) {
    let mut start = 0;
    for i in 0..=s.len() {
        if i == s.len() || s[i] == sep {
            out.push(JStr(strip(&s[start..i]).to_vec()));
            start = i + 1;
        }
    }
}

/// A path-less relation island (a test's assertion, a data space's sample values: `Relation #{ ... }#` with no path),
/// which the engine's walker reparses as `": " + trim(content)` with its `RelationElementsData` grammar at the `#{` (the
/// user's ruling 20 of 2026-11-10, docs/rust-port/PHASE2.md section 6; register S187): refused where that reading stops,
/// the reparse's first line two columns past the `#{`, its later lines that many below the `#{`'s line. `from..to` is the
/// island's content; at the engine's dialect.
pub(crate) fn relation_reparse_refusal(
    ts: &TokenStream<'_>,
    open_tok: usize,
    from: usize,
    to: usize,
) -> Option<JavaError> {
    let src = ts.source();
    let (mut t0, mut t1) = (from, to);
    while t0 < t1 && is_whitespace(src[t0]) {
        t0 += 1;
    }
    while t1 > t0 && is_whitespace(src[t1 - 1]) {
        t1 -= 1;
    }
    let mut text = u(": ");
    text.extend_from_slice(&src[t0..t1]);
    let (stop, unterminated) = relation_elements_scan(&text, 0, text.len())?;
    let (line, column) = (ts.start_line(open_tok) as i32, ts.start_column(open_tok) as i32);
    let before = &text[..stop];
    let (line, mut column) = match before.iter().rposition(|&c| c == u16::from(b'\n')) {
        None => (line, column + 2 + stop as i32),
        Some(nl) => {
            let lines = before.iter().filter(|&&c| c == u16::from(b'\n')).count() as i32;
            (line + lines, (stop - nl) as i32)
        }
    };
    // a table the end of the input cuts: `ParserErrorListener`'s empty span, one column less
    if unterminated {
        column -= 1;
    }
    Some(JavaError::parse(JStr::from("Unexpected token"), line, column).as_refusal())
}

/// The end of the M3 lexer's `VALID_STRING` at `p` (`(Letter | Digit | '_') (Letter | Digit | '_' | '$')*`), none where its
/// token there is another: a number (`INTEGER`, `FLOAT`, `DECIMAL`, which it prefers on a tie) or no word at all (the user's
/// ruling 20 of 2026-11-10, docs/rust-port/PHASE2.md section 6; register S187).
fn m3_valid_string_end(src: &[u16], p: usize, end: usize) -> Option<usize> {
    let ascii = |q: usize, f: fn(&u8) -> bool| q < end && src[q] < 128 && f(&(src[q] as u8));
    let digit = |q: usize| ascii(q, u8::is_ascii_digit);
    let word = |q: usize| ascii(q, |c| c.is_ascii_alphanumeric() || *c == b'_');
    if !word(p) {
        return None;
    }
    let mut valid = p + 1;
    while word(valid) || (valid < end && src[valid] == u16::from(b'$')) {
        valid += 1;
    }
    if !digit(p) {
        return Some(valid);
    }
    // the longest number from `p`: `INTEGER` (digits), `FLOAT` (digits, `'.' digits`, an exponent, a `f`), `DECIMAL`
    // (digits with or without `'.' digits`, an exponent, a required `d`)
    let digits = |mut q: usize| {
        while digit(q) {
            q += 1;
        }
        q
    };
    let at = |q: usize, cs: &[u8]| q < end && cs.iter().any(|&c| src[q] == u16::from(c));
    let exponent = |q: usize| {
        let sign = usize::from(at(q + 1, b"+-"));
        if at(q, b"eE") && digit(q + 1 + sign) {
            digits(q + 1 + sign)
        } else {
            q
        }
    };
    let integer = digits(p);
    let fraction = (at(integer, b".") && digit(integer + 1)).then(|| digits(integer + 1));
    let float = fraction.map(|f| {
        let e = exponent(f);
        if at(e, b"fF") { e + 1 } else { e }
    });
    let decimal = {
        let e = exponent(fraction.unwrap_or(integer));
        at(e, b"dD").then_some(e + 1)
    };
    let number = integer.max(float.unwrap_or(0)).max(decimal.unwrap_or(0));
    (valid > number).then_some(valid)
}

/// Where the engine's `RelationElementsData` grammar stops in `src[start..end]`, none when it reads it all (the user's
/// ruling 18 of 2026-11-09, docs/rust-port/PHASE2.md section 6): `identifier (DOT identifier)* ':'` in the normal lexer's
/// mode, or a `:` with no path before it, then lines of cells (`~[",\r\n;]+`, or a `"..."` with `""` inside) to a `;`. A
/// path's words are the M3 lexer's: a number is no identifier, a `.` before a digit no dot (ruling 20 of 2026-11-10).
pub(crate) fn relation_elements_stop(src: &[u16], start: usize, end: usize) -> Option<usize> {
    relation_elements_scan(src, start, end).map(|(at, _)| at)
}

/// [`relation_elements_stop`]'s place, and whether it is the end of the input inside a table (a `;` missing).
fn relation_elements_scan(src: &[u16], start: usize, end: usize) -> Option<(usize, bool)> {
    let is_nl = |c: u16| c == u16::from(b'\n') || c == u16::from(b'\r');
    let blank = |c: u16| c == u16::from(b' ') || c == u16::from(b'\t');
    let digit_at = |q: usize| q < end && (u16::from(b'0')..=u16::from(b'9')).contains(&src[q]);
    let mut p = start;
    loop {
        // the normal mode, between elements and through a path
        while p < end && (blank(src[p]) || is_nl(src[p])) {
            p += 1;
        }
        if p >= end {
            return None;
        }
        // a `::` is the M3 lexer's PATH_SEPARATOR, no TABLE_START
        let separator = |q: usize| q + 1 < end && src[q] == u16::from(b':') && src[q + 1] == u16::from(b':');
        // `TABLE_START tableCSV`: an element with no path
        let pathless = src[p] == u16::from(b':') && !separator(p);
        if !pathless {
            loop {
                let from = p;
                if src[p] == u16::from(b'\'') {
                    p += 1;
                    while p < end && src[p] != u16::from(b'\'') && !is_nl(src[p]) {
                        p += if src[p] == u16::from(b'\\') { 2 } else { 1 };
                    }
                    if p >= end || src[p] != u16::from(b'\'') {
                        return Some((from, false));
                    }
                    p += 1;
                } else {
                    match m3_valid_string_end(src, p, end) {
                        Some(q) => p = q,
                        None => return Some((from, false)),
                    }
                }
                while p < end && (blank(src[p]) || is_nl(src[p])) {
                    p += 1;
                }
                if p < end && src[p] == u16::from(b'.') {
                    if digit_at(p + 1) {
                        // a FLOAT to the M3 lexer, not a DOT
                        return Some((p, false));
                    }
                    p += 1;
                    while p < end && (blank(src[p]) || is_nl(src[p])) {
                        p += 1;
                    }
                    if p >= end {
                        return Some((p, false));
                    }
                    continue;
                }
                break;
            }
        }
        if p >= end || src[p] != u16::from(b':') || separator(p) {
            return Some((p.min(end), false));
        }
        p += 1;
        while p < end && (blank(src[p]) || is_nl(src[p])) {
            p += 1;
        }
        // the table's mode: a cell, a comma or a line break between cells, a `;` to end it
        let mut cell_closed = false; // a cell has been read and only a separator may follow
        loop {
            if p >= end {
                return Some((end, true));
            }
            let c = src[p];
            if c == u16::from(b';') {
                p += 1;
                break;
            }
            if is_nl(c) {
                while p < end && (is_nl(src[p]) || blank(src[p])) {
                    p += 1;
                }
                cell_closed = false;
                continue;
            }
            if c == u16::from(b',') {
                p += 1;
                cell_closed = false;
                continue;
            }
            if cell_closed {
                return Some((p, false));
            }
            // a quoted cell's leading blanks are its own
            let mut q = p;
            while q < end && blank(src[q]) {
                q += 1;
            }
            if q < end && src[q] == u16::from(b'"') {
                let open = q;
                q += 1;
                let mut closed = false;
                while q < end && !is_nl(src[q]) {
                    if src[q] == u16::from(b'"') {
                        if q + 1 < end && src[q + 1] == u16::from(b'"') {
                            q += 2;
                            continue;
                        }
                        q += 1;
                        closed = true;
                        break;
                    }
                    q += 1;
                }
                if !closed {
                    return Some((open, false));
                }
                while q < end && blank(src[q]) {
                    q += 1;
                }
                p = q;
                cell_closed = true;
                continue;
            }
            while p < end && !is_nl(src[p]) && !matches!(src[p], 0x22 | 0x2C | 0x3B) {
                p += 1;
            }
            cell_closed = true;
        }
    }
}
