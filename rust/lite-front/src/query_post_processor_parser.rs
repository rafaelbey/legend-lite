// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###QueryPostProcessor` (core/'s `parser.section.QueryPostProcessorSectionGrammar`, docs/rust-port/PHASE2.md §4
//! step 4d): the `RelationalMapper` element -- its database, schema and table mapper lists, each optional, in any
//! order, each once. Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R, is_identifier_token, once, unquote_path_iae};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{
    PDatabaseMapper, PRelationalMapper, PSchemaMapper2, PSchemaPointer, PTableMapper2, PTablePointer2,
};
use crate::sections::{split_last, string_value};
use crate::ustr::eq;
use std::collections::HashSet;

use TokenType as T;

/// `QueryPostProcessorSectionGrammar.parseOne`: `RelationalMapper qn ( DatabaseMappers: [...]; SchemaMappers: [...];
/// TableMappers: [...]; )`.
pub fn parse_relational_mapper<'s>(c: &mut impl Cursor<'s>) -> R<PRelationalMapper> {
    if !(is_identifier_token(c.peek()) && eq(&c.safe_text(), "RelationalMapper")) {
        return Err(c.error(jcat!("unsupported ###QueryPostProcessor element: ", c.safe_text())));
    }
    let decl_start = c.pos();
    c.advance()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::PAREN_OPEN)?;
    let mut db_mappers = Vec::new();
    let mut schema_mappers = Vec::new();
    let mut table_mappers = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    let mut last_rank = None;
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let key_start = c.pos();
        let key = c.parse_identifier()?;
        // `DatabaseMappers? SchemaMappers? TableMappers?` in this order, at the engine's dialect, a repeat refused at its
        // key as one out of order (the user's ruling 20 of 2026-11-10, docs/rust-port/PHASE2.md section 6; register S357)
        let rank = ["DatabaseMappers", "SchemaMappers", "TableMappers"]
            .iter()
            .position(|k| eq(&key, k));
        if c.dialect().refuses_lite_extensions() && rank.is_some() {
            if last_rank.is_some_and(|last| rank <= Some(last)) {
                c.set_pos(key_start);
                return Err(c.unexpected());
            }
            last_rank = rank;
        }
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        c.expect(T::BRACKET_OPEN)?;
        if eq(&key, "DatabaseMappers") {
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                c.expect(T::BRACKET_OPEN)?;
                let mut schemas = Vec::new();
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    schemas.push(Box::new(schema_pointer(c)?));
                    c.list_comma()?;
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.expect(T::ARROW)?;
                let to = string_value(c)?;
                db_mappers.push(Box::new(PDatabaseMapper {
                    database_name: JStr(to),
                    schemas,
                }));
                c.list_comma()?;
            }
        } else if eq(&key, "SchemaMappers") {
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                let from = schema_pointer(c)?;
                c.expect(T::ARROW)?;
                schema_mappers.push(Box::new(PSchemaMapper2 {
                    from: Box::new(from),
                    to: JStr(string_value(c)?),
                }));
                c.list_comma()?;
            }
        } else if eq(&key, "TableMappers") {
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                let from = table_pointer(c)?;
                c.expect(T::ARROW)?;
                table_mappers.push(Box::new(PTableMapper2 {
                    from: Box::new(from),
                    to: JStr(string_value(c)?),
                }));
                c.list_comma()?;
            }
        } else {
            return Err(c.error(jcat!("unknown RelationalMapper key: ", key)));
        }
        c.expect(T::BRACKET_CLOSE)?;
        c.expect(T::SEMI_COLON)?;
    }
    let close = c.pos();
    c.expect(T::PAREN_CLOSE)?;
    Ok(PRelationalMapper {
        pkg: JStr(pkg),
        name: JStr(name),
        database_mappers: db_mappers,
        schema_mappers,
        table_mappers,
        source_information: c.span(decl_start, close),
    })
}

/// `schemaPointer`: `db::path.Schema`, its span the whole dotted path.
fn schema_pointer<'s>(c: &mut impl Cursor<'s>) -> R<PSchemaPointer> {
    let s = c.pos();
    let db = unquote_path_iae(&c.parse_qualified_name()?)?;
    c.expect(T::DOT)?;
    let schema = c.parse_identifier()?;
    Ok(PSchemaPointer {
        database: JStr(db),
        schema: JStr(schema),
        source_information: c.span(s, c.pos() - 1),
    })
}

/// `tablePointer`: `db::path.Schema.Table`.
fn table_pointer<'s>(c: &mut impl Cursor<'s>) -> R<PTablePointer2> {
    let s = c.pos();
    let db = unquote_path_iae(&c.parse_qualified_name()?)?;
    c.expect(T::DOT)?;
    let schema = c.parse_identifier()?;
    c.expect(T::DOT)?;
    let table = c.parse_identifier()?;
    Ok(PTablePointer2 {
        database: JStr(db),
        schema: JStr(schema),
        table: JStr(table),
        source_information: c.span(s, c.pos() - 1),
    })
}
