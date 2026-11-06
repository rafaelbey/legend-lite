// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Deephaven` (core/'s `parser.section.DeephavenSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d): the
//! `Deephaven` store (a paren body of typed tables) and the `DeephavenApp` activator, whose body is the
//! function-activator grammar's (`activator_parser`). Java's answers reproduced, each refusal at Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, R};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{Element, PDeephavenColumn, PDeephavenDatabase, PDeephavenDatabasePDeephavenTable};
use crate::sections::head;
use crate::ustr::eq;

use TokenType as T;

/// `COLUMN_TYPES`: a column type's wire `_type` stem.
fn column_type(t: &[u16]) -> Option<&'static str> {
    Some(match String::from_utf16_lossy(t).as_str() {
        "STRING" => "stringType",
        "INT" => "intType",
        "BOOLEAN" => "booleanType",
        "DATETIME" => "dateTimeType",
        "FLOAT" => "floatType",
        "DOUBLE" => "doubleType",
        "TIMESTAMP" => "timestampType",
        "DECIMAL" => "decimalType",
        _ => return None,
    })
}

/// `DeephavenSectionGrammar.parseOne` and `qualifiedNameOf`.
pub fn parse_element<'s>(c: &mut impl Cursor<'s>) -> R<(Element, Vec<u16>)> {
    let kind = c.safe_text();
    if eq(&kind, "Deephaven") {
        let d = parse_store(c)?;
        let path = crate::protocol::qualified_name(&d.pkg.0, &d.name.0);
        return Ok((Element::PDeephavenDatabase(Box::new(d)), path));
    }
    if eq(&kind, "DeephavenApp") {
        let e = crate::activator_parser::parse_element(c, "Deephaven", &["DeephavenApp"])?;
        let path = match &e {
            Element::PFunctionActivator(a) => crate::protocol::qualified_name(&a.pkg.0, &a.name.0),
            _ => unreachable!("a DeephavenApp is an activator"),
        };
        return Ok((e, path));
    }
    Err(c.error(jcat!("unsupported ###Deephaven element: ", kind)))
}

/// `parseStore`: `Deephaven qn ( Table name ( col: TYPE, ... ) ... )`.
fn parse_store<'s>(c: &mut impl Cursor<'s>) -> R<PDeephavenDatabase> {
    let h = head(c, "Deephaven", false)?;
    c.expect(T::PAREN_OPEN)?;
    let mut tables = Vec::new();
    while !c.at_end() && c.peek() != T::PAREN_CLOSE {
        let tk = c.parse_identifier()?;
        if !eq(&tk, "Table") {
            return Err(c.error(jcat!("expected Table, got ", tk)));
        }
        tables.push(Box::new(parse_table(c)?));
    }
    c.expect(T::PAREN_CLOSE)?;
    Ok(PDeephavenDatabase {
        pkg: JStr(h.pkg),
        name: JStr(h.name),
        tables,
        source_information: c.span(h.decl_start, c.pos() - 1),
    })
}

/// `parseTable`: `name ( col: TYPE, col: DECIMAL(p, s), ... )`, precision and scale narrowed to `int` by Java's cast.
fn parse_table<'s>(c: &mut impl Cursor<'s>) -> R<PDeephavenDatabasePDeephavenTable> {
    let table_name = c.parse_identifier()?;
    c.expect(T::PAREN_OPEN)?;
    let mut cols = Vec::new();
    while c.peek() != T::PAREN_CLOSE {
        let col_name = c.parse_identifier()?;
        c.expect(T::COLON)?;
        let col_type = c.parse_identifier()?;
        let Some(wire) = column_type(&col_type) else {
            return Err(c.error(jcat!("unknown Deephaven column type ", col_type)));
        };
        let mut precision = None;
        let mut scale = None;
        if eq(&col_type, "DECIMAL") {
            c.expect(T::PAREN_OPEN)?;
            precision = Some(c.consume_long()? as i32);
            c.expect(T::COMMA)?;
            scale = Some(c.consume_long()? as i32);
            c.expect(T::PAREN_CLOSE)?;
        }
        cols.push(Box::new(PDeephavenColumn { name: JStr(col_name), kind: JStr::from(wire), precision, scale }));
        if !c.match_tok(T::COMMA)? {
            break;
        }
    }
    c.expect(T::PAREN_CLOSE)?;
    Ok(PDeephavenDatabasePDeephavenTable { name: JStr(table_name), columns: cols })
}
