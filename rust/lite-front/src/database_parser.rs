// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! core/'s `parser.DatabaseProtocolParser` (docs/rust-port/PHASE2.md §4 step 4): one `###Relational` `Database` --
//! its includes, schemas, tables (columns, their types, primary keys, milestoning), tabular functions, views, joins
//! and filters -- and the relational operation grammar (`parseOperation`: right-recursive `and`/`or`, chained
//! comparisons, postfix null tests, dynaFuncs, column references, literals, join navigations, function-operation
//! lambdas) that the Mapping grammar borrows for its property mappings ([`operation_at`], [`scoped_operation_at`]).
//!
//! The spans are Java's, quirks included: a comparison's left operand swallows the operator and the right operand, a
//! milestoning dimension's columns sit one to the right, a column reference inside a named schema stretches its table
//! pointer back to the schema's declaration, a `scope(...)` header's table anchors its bare columns.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, base_parse_identifier, is_identifier_token, throw_at, ty_or_slack, unquote_path_iae};
use crate::doors::Dialect;
use crate::jcat;
use crate::jtypes::{JStr, JValue};
use crate::jnum::parse_double;
use crate::lexer::{TokenStream, TokenType};
use crate::nodes::boxed;
use crate::records::{
    PBusinessMilestoning, PBusinessSnapshotMilestoning, PColumnRef, PDatabase, PDateTimeLit, PDbColumn, PDbFilter,
    PDbJoin, PDbSchema, PDbTable, PDbType, PDbView, PDynaFunc, PElemtWithJoins, PIncludedStoreSpec, PJoinPtr,
    PLambdaParam, PMilestoning, PPointer, PProcessingMilestoning, PProcessingSnapshotMilestoning, PRelLambda,
    PRelLiteral, PRelLiteralList, PRelOp, PStereotype, PTablePtr, PTaggedValue, PViewColumnMapping, PViewFilter,
    SourceInfo,
};
use crate::ustr::{eq, index_of_units, last_index_of_str, u};

use TokenType as T;

/// An active `scope([db]seg(.seg)?)` header (`DatabaseProtocolParser.ScopeCtx`): bare identifiers inside are columns
/// of the header's table, whose pointer spans the header's tokens.
#[derive(Clone, Debug)]
pub struct ScopeCtx {
    pub db: Vec<u16>,
    pub schema: Vec<u16>,
    pub table: Vec<u16>,
    pub table_span: SourceInfo,
    pub single_seg: bool,
}

/// Java's `DatabaseProtocolParser` as a cursor: its tokens and position, the database its references resolve under
/// (`""` outside any), the active scope header, the current named schema's declaration span, the stretch anchor a
/// nested navigation element inherits, and the dialect.
pub struct DbCursor<'t, 's> {
    tokens: &'t TokenStream<'s>,
    pos: usize,
    db_fqn: Vec<u16>,
    scope: Option<ScopeCtx>,
    current_schema_decl_span: Option<SourceInfo>,
    schema_stretch_span: Option<SourceInfo>,
    dialect: Dialect,
}

impl<'s> Cursor<'s> for DbCursor<'_, 's> {
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

    /// `parseIdentifier`: relational identifiers admit `"quoted"` spellings, which keep their quotes.
    fn parse_identifier(&mut self) -> R<Vec<u16>> {
        if self.peek() == T::QUOTED_STRING {
            let raw = self.text().to_vec();
            self.advance()?;
            return Ok(raw);
        }
        base_parse_identifier(self)
    }
}

/// `DatabaseProtocolParser.parse(ts, tokenIndex, endOut, dialect)`: one `Database qn ( ... )` at `token_index`, and
/// the token where it ended.
pub fn parse_database(ts: &TokenStream<'_>, token_index: usize, dialect: Dialect) -> R<(PDatabase, usize)> {
    let mut p = DbCursor::new(ts, token_index, Vec::new(), None, dialect);
    let db = p.parse_database()?;
    Ok((db, p.pos))
}

/// `DatabaseProtocolParser.operationAt`: one relational operation at `start`, under `db_fqn` and `schema_ctx` (the
/// Mapping grammar's property mappings), and the token after it.
pub fn operation_at(
    ts: &TokenStream<'_>,
    start: usize,
    db_fqn: &[u16],
    schema_ctx: &[u16],
    dialect: Dialect,
) -> R<(PRelOp, usize)> {
    let mut p = DbCursor::new(ts, start, db_fqn.to_vec(), None, dialect);
    let op = p.parse_operation(schema_ctx)?;
    Ok((op, p.pos))
}

/// `DatabaseProtocolParser.scopedOperationAt`: one operation under an active `scope(...)` header with a table.
pub fn scoped_operation_at(ts: &TokenStream<'_>, start: usize, scope: &ScopeCtx, dialect: Dialect) -> R<(PRelOp, usize)> {
    let mut p = DbCursor::new(ts, start, scope.db.clone(), Some(scope.clone()), dialect);
    let op = p.parse_operation(&u("default"))?;
    Ok((op, p.pos))
}

/// `PRelOp.sourceInformation()`.
pub fn rel_op_span(op: &PRelOp) -> &SourceInfo {
    let s = match op {
        PRelOp::PDynaFunc(f) => &f.source_information,
        PRelOp::PColumnRef(c) => &c.source_information,
        PRelOp::PRelLiteral(l) => &l.source_information,
        PRelOp::PRelLiteralList(l) => &l.source_information,
        PRelOp::PElemtWithJoins(e) => &e.source_information,
        PRelOp::PRelLambda(l) => &l.source_information,
        PRelOp::PLambdaParam(p) => &p.source_information,
    };
    s.as_deref().expect("a parsed node's span")
}

/// `PRelOp`'s record rebuilt with another span (each arm's canonical constructor, every other component kept).
pub fn with_rel_op_span(op: PRelOp, span: SourceInfo) -> PRelOp {
    let si = boxed(span);
    match op {
        PRelOp::PDynaFunc(mut f) => {
            f.source_information = si;
            PRelOp::PDynaFunc(f)
        }
        PRelOp::PColumnRef(mut c) => {
            c.source_information = si;
            PRelOp::PColumnRef(c)
        }
        PRelOp::PRelLiteral(mut l) => {
            l.source_information = si;
            PRelOp::PRelLiteral(l)
        }
        PRelOp::PRelLiteralList(mut l) => {
            l.source_information = si;
            PRelOp::PRelLiteralList(l)
        }
        PRelOp::PElemtWithJoins(mut e) => {
            e.source_information = si;
            PRelOp::PElemtWithJoins(e)
        }
        PRelOp::PRelLambda(mut l) => {
            l.source_information = si;
            PRelOp::PRelLambda(l)
        }
        PRelOp::PLambdaParam(mut p) => {
            p.source_information = si;
            PRelOp::PLambdaParam(p)
        }
    }
}

/// `stretch(base, end)`: `base`'s start, `end`'s end.
fn stretch(base: &SourceInfo, end: &SourceInfo) -> SourceInfo {
    SourceInfo {
        source_id: JStr::default(),
        start_line: base.start_line,
        start_column: base.start_column,
        end_line: end.end_line,
        end_column: end.end_column,
    }
}

/// `withSpanEnd(op, end)`: the node with its span's end replaced by `end`'s -- a join navigation's span stays put.
fn with_span_end(op: PRelOp, end: &SourceInfo) -> PRelOp {
    if matches!(op, PRelOp::PElemtWithJoins(_)) {
        return op;
    }
    let s = stretch(rel_op_span(&op), end);
    with_rel_op_span(op, s)
}

/// `plusOneCols`: the milestoning reparse's `+1` on both span columns.
fn plus_one_cols(sp: SourceInfo) -> SourceInfo {
    SourceInfo {
        source_id: JStr::default(),
        start_line: sp.start_line,
        start_column: sp.start_column + 1,
        end_line: sp.end_line,
        end_column: sp.end_column + 1,
    }
}

/// `String.toUpperCase(Locale.ROOT)` over UTF-16 units: each code point's full upper-case mapping, a lone surrogate
/// kept.
pub(crate) fn to_upper_root(s: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(s.len());
    for r in char::decode_utf16(s.iter().copied()) {
        match r {
            Ok(c) => {
                for up in c.to_uppercase() {
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(up.encode_utf16(&mut buf));
                }
            }
            Err(e) => out.push(e.unpaired_surrogate()),
        }
    }
    out
}

/// `Boolean.parseBoolean`: `"true"`, ignoring case.
fn parse_boolean(s: &[u16]) -> bool {
    s.len() == 4 && s.iter().zip("true".bytes()).all(|(&c, b)| c < 128 && (c as u8).eq_ignore_ascii_case(&b))
}

/// The milestoning block's arguments in Java's `LinkedHashMap` order (a repeated key keeps its first place, takes the
/// last value).
#[derive(Default)]
struct Kv(Vec<(Vec<u16>, Vec<u16>)>);

impl Kv {
    fn put(&mut self, key: Vec<u16>, value: Vec<u16>) {
        if let Some(e) = self.0.iter_mut().find(|e| e.0 == key) {
            e.1 = value;
        } else {
            self.0.push((key, value));
        }
    }
    fn get(&self, key: &str) -> Option<&Vec<u16>> {
        self.0.iter().find(|e| eq(&e.0, key)).map(|e| &e.1)
    }
    fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

impl<'t, 's> DbCursor<'t, 's> {
    fn new(
        tokens: &'t TokenStream<'s>,
        pos: usize,
        db_fqn: Vec<u16>,
        scope: Option<ScopeCtx>,
        dialect: Dialect,
    ) -> DbCursor<'t, 's> {
        DbCursor { tokens, pos, db_fqn, scope, current_schema_decl_span: None, schema_stretch_span: None, dialect }
    }

    /// `parseDatabase`: `documentation? Database <<st>> {tags} qn ( body )`, the body read by a cursor under `qn`.
    fn parse_database(&mut self) -> R<PDatabase> {
        let decl_start = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::DATABASE)?;
        let dec = self.parse_decorations()?;
        let db_dec = self.with_documentation(doc, dec)?;
        let qn = unquote_path_iae(&self.parse_qualified_name()?)?;
        let (pkg, name) = match last_index_of_str(&qn, "::") {
            None => (Vec::new(), qn.clone()),
            Some(cut) => (qn[..cut].to_vec(), qn[cut + 2..].to_vec()),
        };
        let mut inner = DbCursor::new(self.tokens, self.pos, qn, None, self.dialect);
        let db = inner.parse_body(decl_start, pkg, name, db_dec.stereotypes, db_dec.tagged_values)?;
        self.pos = inner.pos;
        Ok(db)
    }

    /// `parseBody`: the members in any order -- includes (plain or typed), schemas, the default schema's tables, views
    /// and tabular functions, joins, filters -- the implicit default schema last, over the whole database's span.
    fn parse_body(
        &mut self,
        decl_start: usize,
        pkg: Vec<u16>,
        name: Vec<u16>,
        db_stereotypes: Vec<Box<PStereotype>>,
        db_tagged_values: Vec<Box<PTaggedValue>>,
    ) -> R<PDatabase> {
        self.expect(T::PAREN_OPEN)?;
        let mut includes = Vec::new();
        let mut default_tables = Vec::new();
        let mut default_views = Vec::new();
        let mut default_tab_fns = Vec::new();
        let mut schemas = Vec::new();
        let mut include_specs = Vec::new();
        let mut joins = Vec::new();
        let mut filters = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let head = if self.peek() == T::DOC_STRING { self.peek_at(1) } else { self.peek() };
            match head {
                T::INCLUDE => {
                    let s = self.pos;
                    self.advance()?;
                    let first = unquote_path_iae(&self.parse_qualified_name()?)?;
                    if index_of_units(&first, &u("::"), 0).is_none()
                        && is_identifier_token(self.peek())
                        && self.peek_at(1) == T::PATH_SEPARATOR
                    {
                        let path = unquote_path_iae(&self.parse_qualified_name()?)?;
                        include_specs.push(Box::new(PIncludedStoreSpec {
                            path: JStr(path),
                            store_type: JStr(first),
                            source_information: self.span(s, self.pos - 1),
                        }));
                    } else {
                        includes.push(Box::new(PPointer {
                            r#type: JStr::from("STORE"),
                            path: JStr(first),
                            source_information: self.span(s, self.pos - 1),
                        }));
                    }
                }
                T::SCHEMA => schemas.push(Box::new(self.parse_schema()?)),
                T::TABLE => default_tables.push(Box::new(self.parse_table()?)),
                T::VIEW => default_views.push(Box::new(self.parse_view(&u("default"))?)),
                T::VALID_STRING => {
                    if !eq(self.text(), "TabularFunction") {
                        return Err(self.error(jcat!("unsupported Database member: '", self.safe_text(), "'")));
                    }
                    default_tab_fns.push(Box::new(self.parse_tabular_function()?));
                }
                T::JOIN => {
                    let s = self.pos;
                    self.advance()?;
                    let jn = self.parse_identifier()?;
                    self.expect(T::PAREN_OPEN)?;
                    let op = self.parse_operation(&u("default"))?;
                    self.expect(T::PAREN_CLOSE)?;
                    joins.push(Box::new(PDbJoin {
                        name: JStr(jn),
                        operation: op,
                        source_information: self.span(s, self.pos - 1),
                    }));
                }
                T::FILTER | T::MULTIGRAIN_FILTER => {
                    let s = self.pos;
                    let ft = if self.peek() == T::MULTIGRAIN_FILTER { "multigrain" } else { "filter" };
                    self.advance()?;
                    let fname = self.parse_identifier()?;
                    self.expect(T::PAREN_OPEN)?;
                    let op = self.parse_operation(&u("default"))?;
                    self.expect(T::PAREN_CLOSE)?;
                    filters.push(Box::new(PDbFilter {
                        filter_type: JStr::from(ft),
                        name: JStr(fname),
                        operation: op,
                        source_information: self.span(s, self.pos - 1),
                    }));
                }
                _ => return Err(self.error(jcat!("unsupported Database member: '", self.safe_text(), "'"))),
            }
        }
        let close = self.pos;
        self.expect(T::PAREN_CLOSE)?;
        let db_span = self.span_of(decl_start, close);
        if !default_tables.is_empty() || !default_views.is_empty() || !default_tab_fns.is_empty() {
            schemas.push(Box::new(PDbSchema {
                name: JStr::from("default"),
                tables: default_tables,
                views: default_views,
                tabular_functions: default_tab_fns,
                stereotypes: Vec::new(),
                tagged_values: Vec::new(),
                source_information: boxed(db_span.clone()),
            }));
        }
        Ok(PDatabase {
            pkg: JStr(pkg),
            name: JStr(name),
            included_stores: includes,
            included_store_specifications: include_specs,
            schemas,
            joins,
            filters,
            stereotypes: db_stereotypes,
            tagged_values: db_tagged_values,
            source_information: boxed(db_span),
        })
    }

    /// `parseSchema`: `documentation? Schema <<st>> {tags} name ( tables, views, tabular functions )` -- a quoted
    /// name kept raw; its name token the current schema's declaration span.
    fn parse_schema(&mut self) -> R<PDbSchema> {
        let s = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::SCHEMA)?;
        let dec = self.parse_decorations()?;
        let dec = self.with_documentation(doc, dec)?;
        let name_tok = self.pos;
        let name = if self.peek() == T::STRING {
            let n = self.text().to_vec();
            self.advance()?;
            n
        } else {
            self.parse_identifier()?
        };
        self.current_schema_decl_span = Some(self.span_of(name_tok, name_tok));
        self.expect(T::PAREN_OPEN)?;
        let mut tables = Vec::new();
        let mut views = Vec::new();
        let mut tab_fns = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let head = if self.peek() == T::DOC_STRING { self.peek_at(1) } else { self.peek() };
            if head == T::TABLE {
                tables.push(Box::new(self.parse_table()?));
            } else if head == T::VIEW {
                views.push(Box::new(self.parse_view(&name)?));
            } else if self.peek() == T::VALID_STRING && eq(self.text(), "TabularFunction") {
                tab_fns.push(Box::new(self.parse_tabular_function()?));
            } else {
                return Err(self.error(jcat!("unsupported Schema member: '", self.safe_text(), "'")));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(PDbSchema {
            name: JStr(name),
            tables,
            views,
            tabular_functions: tab_fns,
            stereotypes: dec.stereotypes,
            tagged_values: dec.tagged_values,
            source_information: self.span(s, self.pos - 1),
        })
    }

    /// `parseTabularFunction`: `TabularFunction TF ( cols )` -- columns, a name and a span.
    fn parse_tabular_function(&mut self) -> R<PDbTable> {
        let s = self.pos;
        self.advance()?;
        let name = self.parse_identifier()?;
        self.expect(T::PAREN_OPEN)?;
        let mut columns = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let c_s = self.pos;
            let col_name = self.parse_identifier()?;
            let ty = self.parse_db_type(&col_name)?;
            let mut nullable = true;
            if self.peek() == T::PRIMARY_KEY || self.peek() == T::NOT_NULL {
                nullable = false;
                self.advance()?;
            }
            columns.push(Box::new(PDbColumn {
                name: JStr(col_name),
                nullable,
                r#type: Box::new(ty),
                stereotypes: Vec::new(),
                tagged_values: Vec::new(),
                source_information: self.span(c_s, self.pos - 1),
            }));
            if !self.match_tok(T::COMMA)? && self.peek() != T::PAREN_CLOSE {
                return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(PDbTable {
            name: JStr(name),
            columns,
            milestoning: Vec::new(),
            primary_key: Vec::new(),
            stereotypes: Vec::new(),
            tagged_values: Vec::new(),
            source_information: self.span(s, self.pos - 1),
        })
    }

    /// `parseTable`: `documentation? Table <<st>> {tags} name ( columns, milestoning? )` -- each column
    /// `documentation? name <<st>> {tags} TYPE (PRIMARY KEY | NOT NULL)?`, a comma between them.
    fn parse_table(&mut self) -> R<PDbTable> {
        let s = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::TABLE)?;
        let dec = self.parse_decorations()?;
        let dec = self.with_documentation(doc, dec)?;
        let name = self.parse_identifier()?;
        self.expect(T::PAREN_OPEN)?;
        let mut columns = Vec::new();
        let mut milestoning = Vec::new();
        let mut primary_key = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            if self.peek() == T::VALID_STRING && eq(self.text(), "milestoning") {
                self.advance()?;
                self.parse_milestoning(&mut milestoning)?;
                continue;
            }
            let c_s = self.pos;
            let col_doc = self.parse_documentation()?;
            let col_name = self.parse_identifier()?;
            let col_dec = self.parse_decorations()?;
            let col_dec = self.with_documentation(col_doc, col_dec)?;
            let ty = self.parse_db_type(&col_name)?;
            let mut nullable = true;
            if self.peek() == T::PRIMARY_KEY || self.peek() == T::NOT_NULL {
                if self.peek() == T::PRIMARY_KEY {
                    primary_key.push(JStr(col_name.clone()));
                }
                nullable = false;
                self.advance()?;
            }
            columns.push(Box::new(PDbColumn {
                name: JStr(col_name),
                nullable,
                r#type: Box::new(ty),
                stereotypes: col_dec.stereotypes,
                tagged_values: col_dec.tagged_values,
                source_information: self.span(c_s, self.pos - 1),
            }));
            if !self.match_tok(T::COMMA)?
                && self.peek() != T::PAREN_CLOSE
                && !(self.peek() == T::VALID_STRING && eq(self.text(), "milestoning"))
            {
                return Err(self.error(jcat!("Unexpected token '", self.safe_text(), "'")));
            }
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(PDbTable {
            name: JStr(name),
            columns,
            milestoning,
            primary_key,
            stereotypes: dec.stereotypes,
            tagged_values: dec.tagged_values,
            source_information: self.span(s, self.pos - 1),
        })
    }

    /// `parseDbType`: a datatype spelling to its wire kind -- sized, precision/scale and plain kinds; the sized and
    /// precise ones require their parameters; an unknown kind refused at its token.
    fn parse_db_type(&mut self, col_name: &[u16]) -> R<PDbType> {
        let type_start = self.pos;
        let kind_word = to_upper_root(&self.parse_identifier()?);
        let kind = match String::from_utf16_lossy(&kind_word).as_str() {
            "INTEGER" | "INT" => "Integer",
            "BIGINT" => "BigInt",
            "SMALLINT" => "SmallInt",
            "TINYINT" => "TinyInt",
            "DOUBLE" => "Double",
            "FLOAT" => "Float",
            "REAL" => "Real",
            "BIT" => "Bit",
            "DATE" => "Date",
            "TIMESTAMP" => "Timestamp",
            "TIME" => "Time",
            "TIMESTAMP_NS" => "TimestampNs",
            "SEMISTRUCTURED" => "SemiStructured",
            "JSON" => "Json",
            "OTHER" | "ARRAY" => "Other",
            "VARCHAR" => "Varchar",
            "CHAR" => "Char",
            "BINARY" => "Binary",
            "VARBINARY" => "Varbinary",
            "DECIMAL" => "Decimal",
            "NUMERIC" => "Numeric",
            _ => {
                return Err(throw_at(self.tokens, type_start, jcat!("unsupported column datatype: ", kind_word)));
            }
        };
        // (a lossy decode matches no kind it should not: every kind is ASCII, and a lone surrogate decodes to U+FFFD)
        let sized = matches!(kind, "Varchar" | "Char" | "Binary" | "Varbinary");
        let precise = matches!(kind, "Decimal" | "Numeric");
        let (mut size, mut precision, mut scale) = (None, None, None);
        if (sized || precise) && self.peek() == T::PAREN_OPEN {
            self.advance()?;
            let mut args = vec![self.consume_long()?];
            while self.match_tok(T::COMMA)? {
                args.push(self.consume_long()?);
            }
            self.expect(T::PAREN_CLOSE)?;
            if precise && args.len() == 2 {
                precision = Some(args[0]);
                scale = Some(args[1]);
            } else if sized && args.len() == 1 {
                size = Some(args[0]);
            } else {
                return Err(self.db_type_arity_error(col_name, &kind_word, sized, type_start));
            }
        } else if sized || precise {
            return Err(self.db_type_arity_error(col_name, &kind_word, sized, type_start));
        }
        Ok(PDbType { kind: JStr::from(kind), size, precision, scale })
    }

    /// `dbTypeArityError`: the engine's refusal, the declaration quoted as ANTLR's `getText()` (the column name and
    /// the type's tokens, no whitespace).
    fn db_type_arity_error(
        &self,
        col_name: &[u16],
        kind_word: &[u16],
        sized: bool,
        type_start: usize,
    ) -> crate::doors::JavaError {
        let mut decl = col_name.to_vec();
        for i in type_start..self.pos {
            decl.extend_from_slice(self.tokens.text(i));
        }
        throw_at(
            self.tokens,
            type_start,
            jcat!(
                "Column data type ",
                kind_word,
                if sized {
                    " requires 1 parameter (size) in declaration '"
                } else {
                    " requires 2 parameters (precision, scale) in declaration '"
                },
                decl,
                "'"
            ),
        )
    }

    /// `parseMilestoning`: `( business(BUS_FROM = c, BUS_THRU = c, ...), processing(...) )` -- each dimension's keys in
    /// the engine's order; a business or snapshot dimension spans its arguments, a processing one itself, both with
    /// the reparse's `+1` columns.
    fn parse_milestoning(&mut self, out: &mut Vec<PMilestoning>) -> R<()> {
        self.expect(T::PAREN_OPEN)?;
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            let s = self.pos;
            let kind = self.parse_identifier()?;
            self.expect(T::PAREN_OPEN)?;
            let args_start = self.pos;
            let mut kv = Kv::default();
            let mut infinity: Option<PDateTimeLit> = None;
            let mut key_order: Vec<Vec<u16>> = Vec::new();
            while !self.at_end() && self.peek() != T::PAREN_CLOSE {
                let key = self.parse_identifier()?;
                key_order.push(key.clone());
                self.expect(T::EQUAL)?;
                if self.peek() == T::DATE {
                    let d_s = self.pos;
                    let raw = self.text();
                    self.advance()?;
                    infinity = Some(PDateTimeLit {
                        value: JStr(raw[1..].to_vec()),
                        source_information: boxed(plus_one_cols(self.span_of(d_s, d_s))),
                    });
                } else {
                    kv.put(key, self.safe_text());
                    self.advance()?;
                }
                self.match_tok(T::COMMA)?;
            }
            self.validate_milestoning_keys(&kind, &key_order)?;
            let args_end = self.pos - 1;
            self.expect(T::PAREN_CLOSE)?;
            let snapshot = kv.contains_key("BUS_SNAPSHOT_DATE") || kv.contains_key("PROCESSING_SNAPSHOT_DATE");
            let span = if eq(&kind, "business") || snapshot {
                plus_one_cols(self.span_of(args_start, args_end))
            } else {
                plus_one_cols(self.span_of(s, self.pos - 1))
            };
            self.match_tok(T::COMMA)?;
            if eq(&kind, "business") {
                if kv.contains_key("BUS_SNAPSHOT_DATE") {
                    out.push(PMilestoning::PBusinessSnapshotMilestoning(Box::new(PBusinessSnapshotMilestoning {
                        snapshot_date: JStr(self.req(&kv, "BUS_SNAPSHOT_DATE")?),
                        source_information: boxed(span),
                    })));
                    continue;
                }
                let from = self.req(&kv, "BUS_FROM")?;
                let thru = self.req(&kv, "BUS_THRU")?;
                out.push(PMilestoning::PBusinessMilestoning(Box::new(PBusinessMilestoning {
                    from: JStr(from),
                    thru: JStr(thru),
                    thru_is_inclusive: kv.get("THRU_IS_INCLUSIVE").is_some_and(|v| parse_boolean(v)),
                    infinity_date: infinity.map(Box::new),
                    source_information: boxed(span),
                })));
            } else if eq(&kind, "processing") {
                if kv.contains_key("PROCESSING_SNAPSHOT_DATE") {
                    out.push(PMilestoning::PProcessingSnapshotMilestoning(Box::new(PProcessingSnapshotMilestoning {
                        snapshot_date: JStr(self.req(&kv, "PROCESSING_SNAPSHOT_DATE")?),
                        source_information: boxed(span),
                    })));
                    continue;
                }
                let r#in = self.req(&kv, "PROCESSING_IN")?;
                let out_ = self.req(&kv, "PROCESSING_OUT")?;
                out.push(PMilestoning::PProcessingMilestoning(Box::new(PProcessingMilestoning {
                    r#in: JStr(r#in),
                    out: JStr(out_),
                    out_is_inclusive: kv.get("OUT_IS_INCLUSIVE").is_some_and(|v| parse_boolean(v)),
                    infinity_date: infinity.map(Box::new),
                    source_information: boxed(span),
                })));
            } else {
                return Err(self.error(jcat!("unsupported milestoning kind: ", kind)));
            }
        }
        self.expect(T::PAREN_CLOSE)
    }

    /// `validateMilestoningKeys`: the engine's ordered key grammar -- `FROM, THRU (, IS_INCLUSIVE)? (, INFINITY_DATE)?`
    /// or `SNAPSHOT_DATE` alone; an unknown, repeated or misplaced key refused with the alternatives that may open.
    fn validate_milestoning_keys(&self, kind: &[u16], keys: &[Vec<u16>]) -> R<()> {
        let business = eq(kind, "business");
        let from = if business { "BUS_FROM" } else { "PROCESSING_IN" };
        let thru = if business { "BUS_THRU" } else { "PROCESSING_OUT" };
        let incl = if business { "THRU_IS_INCLUSIVE" } else { "OUT_IS_INCLUSIVE" };
        let snap = if business { "BUS_SNAPSHOT_DATE" } else { "PROCESSING_SNAPSHOT_DATE" };
        if keys.len() == 1 && eq(&keys[0], snap) {
            return Ok(());
        }
        let expected = [from, thru];
        let mut i = 0;
        for k in keys {
            if i < 2 {
                if !eq(k, expected[i]) {
                    let alternatives = if i == 0 { format!("{from}', '{snap}") } else { thru.to_owned() };
                    return Err(self.error(jcat!("Unexpected token '", k, "'. Valid alternatives: ['", alternatives, "']")));
                }
                i += 1;
                continue;
            }
            if eq(k, incl) && i == 2 {
                i += 1;
                continue;
            }
            if eq(k, "INFINITY_DATE") && (2..=3).contains(&i) {
                i = 4;
                continue;
            }
            return Err(self.error(jcat!("Unexpected token '", k, "'")));
        }
        if i < 2 {
            return Err(self.error(jcat!(
                "milestoning '",
                kind,
                "' needs ",
                from,
                " and ",
                thru,
                " (or ",
                snap,
                ")"
            )));
        }
        Ok(())
    }

    /// `req(kv, key)`: a required milestoning argument.
    fn req(&self, kv: &Kv, key: &str) -> R<Vec<u16>> {
        match kv.get(key) {
            Some(v) => Ok(v.clone()),
            None => Err(self.error(jcat!("milestoning needs ", key))),
        }
    }

    /// `parseView(schemaCtx)`: `documentation? View <<st>> {tags} name ( ~distinct? ~filter? ~groupBy(...)? col: op
    /// PRIMARY KEY?, ... )`, its operations under `schema_ctx`.
    fn parse_view(&mut self, schema_ctx: &[u16]) -> R<PDbView> {
        let s = self.pos;
        let doc = self.parse_documentation()?;
        self.expect(T::VIEW)?;
        let dec = self.parse_decorations()?;
        let dec = self.with_documentation(doc, dec)?;
        let name = self.parse_identifier()?;
        self.expect(T::PAREN_OPEN)?;
        let mut distinct = false;
        let mut filter: Option<PViewFilter> = None;
        let mut group_by = Vec::new();
        let mut mappings = Vec::new();
        let mut primary_key = Vec::new();
        while !self.at_end() && self.peek() != T::PAREN_CLOSE {
            if self.peek() == T::DISTINCT_CMD {
                self.advance()?;
                distinct = true;
                continue;
            }
            if self.peek() == T::FILTER_CMD {
                let t_s = self.pos;
                self.advance()?;
                let mut fdb: Option<Vec<u16>> = None;
                let mut fjoins = Vec::new();
                if self.peek() == T::AT {
                    return Err(self.error_str(
                        "join-mediated ~filter needs a source [DB] pointer before the join sequence (~filter [DB] \
                         @Join | [DB] FilterName)",
                    ));
                }
                if self.peek() == T::BRACKET_OPEN {
                    self.advance()?;
                    let first_db = unquote_path_iae(&self.parse_qualified_name()?)?;
                    self.expect(T::BRACKET_CLOSE)?;
                    if self.peek() == T::AT {
                        while self.peek() == T::AT {
                            let j_s = self.pos;
                            self.advance()?;
                            let jn = self.parse_identifier()?;
                            fjoins.push(Box::new(PJoinPtr {
                                db: Some(JStr(first_db.clone())),
                                join_type: None,
                                name: JStr(jn),
                                source_information: self.span(j_s, self.pos - 1),
                            }));
                            if self.peek() == T::GREATER_THAN {
                                self.advance()?;
                            }
                        }
                        self.expect(T::PIPE)?;
                        if self.peek() != T::BRACKET_OPEN {
                            return Err(self.error_str(
                                "join-mediated ~filter needs a target [DB] pointer after the '|' (~filter [DB] @Join \
                                 | [DB] FilterName)",
                            ));
                        }
                        self.expect(T::BRACKET_OPEN)?;
                        fdb = Some(unquote_path_iae(&self.parse_qualified_name()?)?);
                        self.expect(T::BRACKET_CLOSE)?;
                    } else {
                        fdb = Some(first_db);
                    }
                }
                let fname = self.parse_identifier()?;
                filter = Some(PViewFilter {
                    db: fdb.map(JStr),
                    name: JStr(fname),
                    joins: fjoins,
                    source_information: self.span(t_s, self.pos - 1),
                });
                continue;
            }
            if self.peek() == T::GROUP_BY_CMD {
                self.advance()?;
                group_by = Vec::new();
                self.expect(T::PAREN_OPEN)?;
                while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                    group_by.push(self.parse_operation(schema_ctx)?);
                    self.match_tok(T::COMMA)?;
                }
                self.expect(T::PAREN_CLOSE)?;
                continue;
            }
            let m_s = self.pos;
            let col_name = self.parse_identifier()?;
            self.expect(T::COLON)?;
            let op = self.parse_operation(schema_ctx)?;
            if self.peek() == T::PRIMARY_KEY {
                self.advance()?;
                primary_key.push(JStr(col_name.clone()));
            }
            mappings.push(Box::new(PViewColumnMapping {
                name: JStr(col_name),
                operation: op,
                source_information: self.span(m_s, self.pos - 1),
            }));
            self.match_tok(T::COMMA)?;
        }
        self.expect(T::PAREN_CLOSE)?;
        Ok(PDbView {
            name: JStr(name),
            column_mappings: mappings,
            stereotypes: dec.stereotypes,
            tagged_values: dec.tagged_values,
            distinct,
            filter: filter.map(Box::new),
            group_by,
            primary_key,
            source_information: self.span(s, self.pos - 1),
        })
    }

    // ---------------------------------------------------------------- operations: or > and > comparison > atom

    /// `parseOperation`: `and`/`or` right-recursive with no precedence (`x and y or z` is `and(x, or(y, z))`), a run
    /// of the same operator flattened n-ary under the first one's span, a different one on the right wrapped in
    /// `group`; each node spans its operator token through the right operand.
    pub fn parse_operation(&mut self, schema_ctx: &[u16]) -> R<PRelOp> {
        let left = self.parse_comparison(schema_ctx)?;
        let Some(op) = self.bool_op_here() else {
            return Ok(left);
        };
        let op_tok = self.pos;
        self.advance()?;
        let mut rest = self.parse_operation(schema_ctx)?;
        let span = self.span(op_tok, self.pos - 1);
        if let PRelOp::PDynaFunc(df) = &rest
            && eq(&df.func_name.0, op)
        {
            let PRelOp::PDynaFunc(df) = rest else { unreachable!("matched above") };
            let mut params = Vec::with_capacity(df.parameters.len() + 1);
            params.push(left);
            params.extend(df.parameters);
            return Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
                func_name: JStr::from(op),
                parameters: params,
                source_information: span,
            })));
        }
        if let PRelOp::PDynaFunc(df) = &rest
            && (eq(&df.func_name.0, "and") || eq(&df.func_name.0, "or"))
        {
            let si = df.source_information.clone();
            rest = PRelOp::PDynaFunc(Box::new(PDynaFunc {
                func_name: JStr::from("group"),
                parameters: vec![rest],
                source_information: si,
            }));
        }
        Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
            func_name: JStr::from(op),
            parameters: vec![left, rest],
            source_information: span,
        })))
    }

    /// `boolOpHere`: `and` or `or` (their keyword tokens, or the plain words).
    fn bool_op_here(&self) -> Option<&'static str> {
        if self.peek() == T::RELATIONAL_AND || (self.peek() == T::VALID_STRING && eq(self.text(), "and")) {
            return Some("and");
        }
        if self.peek() == T::RELATIONAL_OR || (self.peek() == T::VALID_STRING && eq(self.text(), "or")) {
            return Some("or");
        }
        None
    }

    /// `parseComparison`: an atom, its postfix null tests (the operand swallowing each), then one comparison chaining
    /// right-recursively -- the left operand's span stretched to the comparison's end, the node spanning the operator
    /// through the right operand.
    fn parse_comparison(&mut self, schema_ctx: &[u16]) -> R<PRelOp> {
        let mut left = self.parse_atom(schema_ctx)?;
        while self.peek() == T::IS_NULL || self.peek() == T::IS_NOT_NULL {
            let nf = if self.peek() == T::IS_NULL { "isNull" } else { "isNotNull" };
            let n_tok = self.pos;
            self.advance()?;
            let op_span = self.span_of(n_tok, n_tok);
            left = PRelOp::PDynaFunc(Box::new(PDynaFunc {
                func_name: JStr::from(nf),
                parameters: vec![with_span_end(left, &op_span)],
                source_information: boxed(op_span),
            }));
        }
        let func = match self.peek() {
            T::EQUAL => "equal",
            T::NOT_EQUAL => "notEqualAnsi",
            T::TEST_NOT_EQUAL => "notEqual",
            T::GREATER_THAN => "greaterThan",
            T::GREATER_OR_EQUAL => "greaterThanEqual",
            T::LESS_THAN => "lessThan",
            T::LESS_OR_EQUAL => "lessThanEqual",
            _ => return Ok(left),
        };
        let op_tok = self.pos;
        self.advance()?;
        let right = self.parse_comparison(schema_ctx)?;
        let comp_span = self.span_of(op_tok, self.pos - 1);
        let left = with_span_end(left, &comp_span);
        Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
            func_name: JStr::from(func),
            parameters: vec![left, right],
            source_information: boxed(comp_span),
        })))
    }

    /// `dbOrNull`: no database outside one (a mapping's bare references).
    fn db_or_null(&self) -> Option<JStr> {
        if self.db_fqn.is_empty() { None } else { Some(JStr(self.db_fqn.clone())) }
    }

    /// `operationUnder(db, schemaCtx)`: a whole operation resolved under `db` -- a join navigation's terminal.
    fn operation_under(&mut self, db: &[u16], schema_ctx: &[u16]) -> R<PRelOp> {
        self.under(db, schema_ctx, true)
    }

    /// `under(db, schemaCtx, whole)`: in place when `db` is this cursor's and no schema stretch applies, else by a
    /// cursor under `db` that inherits the scope and the current schema's declaration, its stretch anchor that one.
    fn under(&mut self, db: &[u16], schema_ctx: &[u16], whole: bool) -> R<PRelOp> {
        let stretch = if eq(schema_ctx, "default") { None } else { self.current_schema_decl_span.clone() };
        if db == self.db_fqn.as_slice() && stretch.is_none() {
            return if whole { self.parse_operation(schema_ctx) } else { self.parse_atom(schema_ctx) };
        }
        let mut p = DbCursor::new(self.tokens, self.pos, db.to_vec(), self.scope.clone(), self.dialect);
        p.schema_stretch_span = stretch;
        p.current_schema_decl_span = self.current_schema_decl_span.clone();
        let e = if whole { p.parse_operation(schema_ctx)? } else { p.parse_atom(schema_ctx)? };
        self.pos = p.pos;
        Ok(e)
    }

    /// `bracketLeadsToAt`: the `[` opens a database pointer followed by an `@` navigation (`[db]@J`, or `[db] (TYPE)
    /// @J`).
    fn bracket_leads_to_at(&self) -> bool {
        let ts = self.tokens;
        let mut i = self.pos + 1;
        let mut depth = 1;
        while i < ts.count() && depth > 0 {
            let t = ts.ty(i);
            if t == T::BRACKET_OPEN {
                depth += 1;
            } else if t == T::BRACKET_CLOSE {
                depth -= 1;
            }
            i += 1;
        }
        if i + 3 < ts.count() && ts.ty(i) == T::PAREN_OPEN && ts.ty(i + 2) == T::PAREN_CLOSE && ts.ty(i + 3) == T::AT {
            return true;
        }
        i < ts.count() && ts.ty(i) == T::AT
    }

    /// `parseJoinNav(db, schemaCtx, s, firstType)`: `@J [> (TYPE)? [db2]? @K]* [| operation]` -- each pointer spanning
    /// its `@` through its name, a type riding the next pointer, the terminal a whole operation under the last
    /// pointer's database.
    fn parse_join_nav(
        &mut self,
        db: &[u16],
        schema_ctx: &[u16],
        s: usize,
        first_type: Option<Vec<u16>>,
    ) -> R<PRelOp> {
        let mut join_ptrs = Vec::new();
        let mut cur_db = db.to_vec();
        let mut pending_type = first_type;
        while self.peek() == T::AT {
            let j_s = self.pos;
            self.advance()?;
            let jn = self.parse_identifier()?;
            join_ptrs.push(Box::new(PJoinPtr {
                db: if cur_db.is_empty() { None } else { Some(JStr(cur_db.clone())) },
                join_type: pending_type.take().map(JStr),
                name: JStr(jn),
                source_information: self.span(j_s, self.pos - 1),
            }));
            if self.peek() == T::GREATER_THAN {
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
            } else {
                break;
            }
        }
        let mut elem = None;
        if self.peek() == T::PIPE {
            self.advance()?;
            elem = Some(self.operation_under(&cur_db, schema_ctx)?);
        }
        Ok(PRelOp::PElemtWithJoins(Box::new(PElemtWithJoins {
            joins: join_ptrs,
            relational_element: elem,
            source_information: self.span(s, self.pos - 1),
        })))
    }

    /// `looksLikeDbPointer`: at a `[`, a database pointer (one qualified name, then `]` and a token that continues an
    /// operation) rather than an argument array.
    fn looks_like_db_pointer(&self) -> bool {
        let ts = self.tokens;
        let mut i = self.pos + 1;
        if !is_identifier_token(ty_or_slack(ts, i)) {
            return false;
        }
        i += 1;
        while i + 1 < ts.count() && ts.ty(i) == T::PATH_SEPARATOR && is_identifier_token(ts.ty(i + 1)) {
            i += 2;
        }
        if i >= ts.count() || ts.ty(i) != T::BRACKET_CLOSE {
            return false;
        }
        let after = if i + 1 < ts.count() { ts.ty(i + 1) } else { T::EOF };
        is_identifier_token(after) || after == T::QUOTED_STRING || after == T::AT || after == T::PAREN_OPEN
    }

    /// `parseFunctionArgument`: an operation, or a lambda (`x | op`, `(x, y | op)`) spanning its whole rule.
    fn parse_function_argument(&mut self, schema_ctx: &[u16]) -> R<PRelOp> {
        let s = self.pos;
        if is_identifier_token(self.peek()) && self.peek_at(1) == T::PIPE {
            let param = self.parse_identifier()?;
            self.expect(T::PIPE)?;
            let body = self.parse_operation(schema_ctx)?;
            return Ok(PRelOp::PRelLambda(Box::new(PRelLambda {
                parameter_names: vec![JStr(param)],
                body,
                source_information: self.span(s, self.pos - 1),
            })));
        }
        if self.peek() == T::PAREN_OPEN
            && is_identifier_token(self.peek_at(1))
            && (self.peek_at(2) == T::PIPE || self.peek_at(2) == T::COMMA)
        {
            let save = self.pos;
            self.advance()?;
            let mut params = vec![JStr(self.parse_identifier()?)];
            while self.match_tok(T::COMMA)? {
                if !is_identifier_token(self.peek()) {
                    self.pos = save;
                    return self.parse_operation(schema_ctx);
                }
                params.push(JStr(self.parse_identifier()?));
            }
            if self.peek() != T::PIPE {
                self.pos = save;
                return self.parse_operation(schema_ctx);
            }
            self.advance()?;
            let body = self.parse_operation(schema_ctx)?;
            self.expect(T::PAREN_CLOSE)?;
            return Ok(PRelOp::PRelLambda(Box::new(PRelLambda {
                parameter_names: params,
                body,
                source_information: self.span(s, self.pos - 1),
            })));
        }
        self.parse_operation(schema_ctx)
    }

    fn table_ptr(
        &self,
        database: Option<JStr>,
        schema: Vec<u16>,
        table: Vec<u16>,
        span: SourceInfo,
    ) -> Box<PTablePtr> {
        Box::new(PTablePtr {
            main_table_db: database.clone(),
            database,
            schema: JStr(schema),
            table: JStr(table),
            source_information: boxed(span),
            written_type: None,
        })
    }

    /// `parseAtom`: a `group`ed parenthesis, a string, a (negated) number, a lambda parameter, `{target}.col`, a
    /// bracket-qualified navigation or reference or dynaFunc, an argument array, a navigation, a dynaFunc, or a
    /// dotted column reference -- the scope header's table where one is active.
    fn parse_atom(&mut self, schema_ctx: &[u16]) -> R<PRelOp> {
        let s = self.pos;
        if self.peek() == T::PAREN_OPEN {
            let g_s = self.pos;
            self.advance()?;
            let inner = self.parse_operation(schema_ctx)?;
            self.expect(T::PAREN_CLOSE)?;
            return Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
                func_name: JStr::from("group"),
                parameters: vec![inner],
                source_information: self.span(g_s, self.pos - 1),
            })));
        }
        if self.peek() == T::STRING {
            let quoted = self.text();
            self.advance()?;
            let v = self.unquote_and_unescape(quoted)?;
            return Ok(literal(JValue::String(JStr(v)), self.span(s, s)));
        }
        if self.peek() == T::MINUS {
            self.advance()?;
            let num_tok = self.pos;
            if self.peek() == T::INTEGER {
                let v = -self.consume_long()?;
                return Ok(literal(JValue::Long(v), self.span(s, num_tok)));
            }
            let v = -parse_double(self.slack_text())
                .map_err(|m| crate::doors::JavaError::java("java.lang.NumberFormatException", m))?;
            self.expect(T::FLOAT)?;
            return Ok(literal(JValue::Double(v), self.span(s, num_tok)));
        }
        if self.peek() == T::INTEGER {
            let v = self.consume_long()?;
            return Ok(literal(JValue::Long(v), self.span(s, s)));
        }
        if self.peek() == T::FLOAT {
            let v = parse_double(self.text())
                .map_err(|m| crate::doors::JavaError::java("java.lang.NumberFormatException", m))?;
            self.advance()?;
            return Ok(literal(JValue::Double(v), self.span(s, s)));
        }
        if self.peek() == T::DOLLAR {
            self.advance()?;
            let name = self.parse_identifier()?;
            return Ok(PRelOp::PLambdaParam(Box::new(PLambdaParam {
                name: JStr(name),
                source_information: self.span(s, self.pos - 1),
            })));
        }
        if self.peek() == T::TARGET {
            self.advance()?;
            self.expect(T::DOT)?;
            let col = self.parse_identifier()?;
            let span = self.span(s, self.pos - 1);
            let table = self.table_ptr(self.db_or_null(), schema_ctx.to_vec(), u("{target}"), self.span_of(s, s));
            return Ok(PRelOp::PColumnRef(Box::new(PColumnRef {
                column: JStr(col),
                table,
                table_alias: JStr::from("{target}"),
                source_information: span,
            })));
        }
        if self.peek() == T::BRACKET_OPEN && self.looks_like_db_pointer() && self.bracket_leads_to_at() {
            let b_s = self.pos;
            self.advance()?;
            let db = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::BRACKET_CLOSE)?;
            let mut first_type = None;
            if self.peek() == T::PAREN_OPEN {
                self.advance()?;
                let t = self.parse_identifier()?;
                self.validate_join_type(&t)?;
                first_type = Some(t);
                self.expect(T::PAREN_CLOSE)?;
            }
            return self.parse_join_nav(&db, schema_ctx, b_s, first_type);
        }
        if self.peek() == T::BRACKET_OPEN && self.looks_like_db_pointer() {
            self.advance()?;
            let db = unquote_path_iae(&self.parse_qualified_name()?)?;
            self.expect(T::BRACKET_CLOSE)?;
            if self.peek() == T::VALID_STRING && self.peek_at(1) == T::PAREN_OPEN {
                let mut p = DbCursor::new(self.tokens, self.pos, db, self.scope.clone(), self.dialect);
                p.current_schema_decl_span = self.current_schema_decl_span.clone();
                p.schema_stretch_span = self.schema_stretch_span.clone();
                let f = p.parse_atom(schema_ctx)?;
                self.pos = p.pos;
                let PRelOp::PDynaFunc(f) = f else { unreachable!("a name and '(' parse as a dynaFunc") };
                let br = self.span_of(s, s);
                let end = f.source_information.as_deref().expect("a parsed node's span");
                let span = stretch(&br, end);
                return Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
                    func_name: f.func_name,
                    parameters: f.parameters,
                    source_information: boxed(span),
                })));
            }
            if let Some(scope) = self.scope.clone()
                && self.peek() == T::VALID_STRING
                && self.peek_at(1) != T::DOT
            {
                let col = self.parse_identifier()?;
                let table = Box::new(PTablePtr {
                    database: Some(JStr(db.clone())),
                    main_table_db: Some(JStr(db)),
                    schema: JStr(scope.schema),
                    table: JStr(scope.table.clone()),
                    source_information: boxed(scope.table_span),
                    written_type: None,
                });
                return Ok(PRelOp::PColumnRef(Box::new(PColumnRef {
                    column: JStr(col),
                    table,
                    table_alias: JStr(scope.table),
                    source_information: self.span(s, self.pos - 1),
                })));
            }
            let first_tok = self.pos;
            let mut tbl_end_tok = self.pos;
            let mut schema = u("default");
            let mut table = self.parse_identifier()?;
            self.expect(T::DOT)?;
            let mut column = self.parse_identifier()?;
            if self.peek() == T::DOT {
                self.advance()?;
                schema = table;
                table = column;
                tbl_end_tok = self.pos - 2;
                column = self.parse_identifier()?;
            }
            let ptr = self.table_ptr(Some(JStr(db)), schema, table.clone(), self.span_of(first_tok, tbl_end_tok));
            return Ok(PRelOp::PColumnRef(Box::new(PColumnRef {
                column: JStr(column),
                table: ptr,
                table_alias: JStr(table),
                source_information: self.span(s, self.pos - 1),
            })));
        }
        if self.peek() == T::BRACKET_OPEN {
            let l_s = self.pos;
            self.advance()?;
            let mut items = Vec::new();
            while self.peek() != T::BRACKET_CLOSE && !self.at_end() {
                items.push(self.parse_function_argument(schema_ctx)?);
                self.match_tok(T::COMMA)?;
            }
            self.expect(T::BRACKET_CLOSE)?;
            return Ok(PRelOp::PRelLiteralList(Box::new(PRelLiteralList {
                values: items,
                source_information: self.span(l_s, self.pos - 1),
            })));
        }
        if self.peek() == T::AT {
            let db = self.db_fqn.clone();
            return self.parse_join_nav(&db, schema_ctx, self.pos, None);
        }
        let first = self.parse_identifier()?;
        if let Some(scope) = self.scope.clone()
            && self.peek() != T::PAREN_OPEN
            && self.peek() != T::DOT
        {
            let table = Box::new(PTablePtr {
                database: Some(JStr(self.db_fqn.clone())),
                main_table_db: Some(JStr(self.db_fqn.clone())),
                schema: JStr(scope.schema),
                table: JStr(scope.table.clone()),
                source_information: boxed(scope.table_span),
                written_type: None,
            });
            return Ok(PRelOp::PColumnRef(Box::new(PColumnRef {
                column: JStr(first),
                table,
                table_alias: JStr(scope.table),
                source_information: self.span(s, s),
            })));
        }
        if self.peek() == T::PAREN_OPEN {
            self.advance()?;
            let mut args = Vec::new();
            while self.peek() != T::PAREN_CLOSE && !self.at_end() {
                args.push(self.parse_function_argument(schema_ctx)?);
                self.match_tok(T::COMMA)?;
            }
            self.expect(T::PAREN_CLOSE)?;
            return Ok(PRelOp::PDynaFunc(Box::new(PDynaFunc {
                func_name: JStr(first),
                parameters: args,
                source_information: self.span(s, self.pos - 1),
            })));
        }
        if self.peek() != T::DOT {
            return Err(self.error(jcat!("Missing table or alias for column '", first, "'")));
        }
        self.expect(T::DOT)?;
        let mut tbl_end = self.pos - 2;
        let second = self.parse_identifier()?;
        let mut schema = schema_ctx.to_vec();
        let mut table = first.clone();
        let mut column = second.clone();
        let mut explicit_schema = false;
        if self.peek() == T::DOT {
            self.advance()?;
            schema = first;
            table = second;
            tbl_end = self.pos - 2;
            column = self.parse_identifier()?;
            explicit_schema = true;
        }
        if let Some(scope) = &self.scope
            && scope.single_seg
            && !explicit_schema
        {
            schema = scope.table.clone();
        }
        let span = self.span(s, self.pos - 1);
        let mut tbl_span = self.span_of(s, tbl_end);
        let decl_anchor = match &self.schema_stretch_span {
            Some(a) => Some(a.clone()),
            None if !eq(schema_ctx, "default") => self.current_schema_decl_span.clone(),
            None => None,
        };
        if let Some(anchor) = decl_anchor.filter(|_| !explicit_schema) {
            tbl_span = stretch(&anchor, &tbl_span);
        } else if let Some(scope) = self.scope.as_ref().filter(|_| !explicit_schema) {
            tbl_span = stretch(&scope.table_span, &tbl_span);
        }
        let ptr = self.table_ptr(self.db_or_null(), schema, table.clone(), tbl_span);
        Ok(PRelOp::PColumnRef(Box::new(PColumnRef {
            column: JStr(column),
            table: ptr,
            table_alias: JStr(table),
            source_information: span,
        })))
    }
}

fn literal(value: JValue, span: Option<Box<SourceInfo>>) -> PRelOp {
    PRelOp::PRelLiteral(Box::new(PRelLiteral { value, source_information: span }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    #[test]
    fn a_comparison_s_left_operand_swallows_the_right() {
        let text = u("Database d ( Table T (ID INT PRIMARY KEY, NAME VARCHAR(20)) Join j(T.ID = T.NAME) )");
        let ts = tokenize(&text);
        let (db, end) = parse_database(&ts, 0, Dialect::LegendLite).expect("parses");
        assert_eq!(end, ts.count());
        assert_eq!(db.schemas.len(), 1, "the implicit default schema");
        let PRelOp::PDynaFunc(f) = &db.joins[0].operation else { panic!("a comparison") };
        assert_eq!(f.func_name, "equal");
        let left = rel_op_span(&f.parameters[0]);
        let node = f.source_information.as_deref().unwrap();
        assert_eq!((left.end_line, left.end_column), (node.end_line, node.end_column));
    }

    #[test]
    fn an_unknown_type_is_refused_at_its_token() {
        let text = u("Database d ( Table T (ID WIBBLE) )");
        let ts = tokenize(&text);
        let e = parse_database(&ts, 0, Dialect::LegendLite).unwrap_err();
        assert_eq!(e.message, Some(JStr::from("[1:26] unsupported column datatype: WIBBLE")));
    }
}
