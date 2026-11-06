// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Runtime` (core/'s `parser.section.RuntimeSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4c): `Runtime` and
//! `SingleConnectionRuntime` elements -- mappings, store connections (pointers or embedded `#{ <Flavor> {...} }#`
//! islands, which the Connection grammar reads re-lexed: `connection_parser::parse_island_value`), connection stores --
//! and the anonymous runtime a service embeds (`#{ ... }#`, re-lexed with the walker offsets: [`parse_embedded_body`]).
//!
//! Java's answers reproduced: the key counts are field occurrences, a duplicate refused at the element (the
//! `connectionStores` once-message naming the protocol's field, `connectionPointerStores`); a store group with no
//! connection is dropped; a connection island nested in a re-lexed runtime composes its offsets with the runtime's --
//! its column offset only when the island starts on the runtime island's first line.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::connection_parser::parse_island_value;
use crate::cursor::{Cursor, IslandCursor, R, throw_at, unquote_path_iae};
use crate::database_parser::to_upper_root;
use crate::doors::Dialect;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::{TokenType, tokenize};
use crate::records::{
    PConnectionPointer, PConnectionStores, PConnectionValue, PEmbeddedRuntime, PIdentifiedConnection, PPointer,
    PRuntime, PStoreConnections, PStorePointer,
};
use crate::sections::{scan_island, split_last};
use crate::ustr::eq;

use TokenType as T;

/// `RuntimeSectionGrammar.parseElement`: `Runtime | SingleConnectionRuntime qn { mappings: [...]; connections: [...];
/// connectionStores: [...]; }`, `mappings` required.
pub fn parse_runtime<'s>(c: &mut impl Cursor<'s>) -> R<PRuntime> {
    let decl_start = c.pos();
    let single = c.peek() == T::SINGLE_CONNECTION_RUNTIME;
    c.advance()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut body = Body::default();
    let mappings_keys = parse_body_keys(c, single, &qn, Some(T::BRACE_CLOSE), &mut body, decl_start)?;
    if mappings_keys == 0 {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'mappings' is required")));
    }
    c.expect(T::BRACE_CLOSE)?;
    Ok(PRuntime {
        pkg: JStr(pkg),
        name: JStr(name),
        single,
        mappings: body.mappings,
        connections: body.connections,
        connection_stores: body.connection_stores,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// A runtime body's three lists.
#[derive(Default)]
struct Body {
    mappings: Vec<Box<PPointer>>,
    connections: Vec<Box<PStoreConnections>>,
    connection_stores: Vec<Box<PConnectionStores>>,
}

/// `parseConnections`: `[ store: [ id: <connection pointer or island>, ... ], ... ];`.
fn parse_connections<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PStoreConnections>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let s_start = c.pos();
        let store = unquote_path_iae(&c.parse_qualified_name()?)?;
        let store_ptr = Box::new(PPointer {
            r#type: JStr::from("STORE"),
            path: JStr(store),
            source_information: c.span(s_start, c.pos() - 1),
        });
        c.expect(T::COLON)?;
        c.expect(T::BRACKET_OPEN)?;
        let mut ids = Vec::new();
        while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
            let id_start = c.pos();
            let id = c.parse_identifier()?;
            c.expect(T::COLON)?;
            let connection = if c.peek() == T::ISLAND_OPEN {
                parse_embedded_connection(c)?
            } else {
                let c_start = c.pos();
                let conn = unquote_path_iae(&c.parse_qualified_name()?)?;
                PConnectionValue::PConnectionPointer(Box::new(PConnectionPointer {
                    connection: JStr(conn),
                    source_information: c.span(c_start, c.pos() - 1),
                }))
            };
            ids.push(Box::new(PIdentifiedConnection {
                id: JStr(id),
                connection,
                source_information: c.span(id_start, c.pos() - 1),
            }));
            c.match_tok(T::COMMA)?;
        }
        let group_end = c.pos();
        c.expect(T::BRACKET_CLOSE)?;
        if !ids.is_empty() {
            // the engine drops a store group with no connection
            out.push(Box::new(PStoreConnections {
                store: store_ptr,
                store_connections: ids,
                source_information: c.span(s_start, group_end),
            }));
        }
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `parseEmbeddedConnection`: a `#{ <Flavor> {...} }#` island, its text handed to the Connection grammar with the
/// walker offsets of its first content token -- composed with the host's own when the host is a runtime island's
/// cursor (its column offset only for an island on that island's first line).
fn parse_embedded_connection<'s>(c: &mut impl Cursor<'s>) -> R<PConnectionValue> {
    let (emb_start, emb_end) = scan_island(c)?;
    c.expect(T::ISLAND_END)?;
    let emb = c.reconstruct_text(emb_start, emb_end);
    let ts = c.tokens();
    let mut base_line = ts.start_line(emb_start) as i32;
    let mut base_col = ts.start_column(emb_start) as i32;
    if let Some((line_offset, col_offset)) = c.island_offsets() {
        if ts.start_line(emb_start) == 1 {
            base_col += col_offset;
        }
        base_line += line_offset;
    }
    parse_island_value(&emb, base_line, base_col, c.dialect())
}

/// `parseBodyKeys`: the runtime body's key loop (`mappings`, `connections`, `connectionStores`; a single-connection
/// runtime's `connection`), shared by the section element and a service's embedded island (`close` `None`: to the
/// island's end). Answers how many times `mappings` appeared.
fn parse_body_keys<'s>(
    c: &mut impl Cursor<'s>,
    single: bool,
    qn: &[u16],
    close: Option<TokenType>,
    body: &mut Body,
    decl_start: usize,
) -> R<i32> {
    let (mut seen_mappings, mut seen_connections, mut seen_conn_stores) = (0, 0, 0);
    while !c.at_end() && close.is_none_or(|t| c.peek() != t) {
        let key = c.peek();
        let key_text = c.safe_text();
        c.advance()?;
        c.expect(T::COLON)?;
        let once =
            |what: &str| throw_at(c.tokens(), decl_start, jcat!("Field '", what, "' should be specified only once"));
        if key == T::MAPPINGS {
            seen_mappings += 1;
            if seen_mappings > 1 {
                return Err(once("mappings"));
            }
        }
        if key == T::CONNECTIONS && !single {
            seen_connections += 1;
            if seen_connections > 1 {
                return Err(once("connections"));
            }
        }
        let conn_stores = eq(&key_text, "connectionStores");
        if conn_stores && !single {
            seen_conn_stores += 1;
            if seen_conn_stores > 1 {
                return Err(once("connectionPointerStores"));
            }
        }
        if key == T::MAPPINGS {
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                let m_start = c.pos();
                let path = unquote_path_iae(&c.parse_qualified_name()?)?;
                body.mappings.push(Box::new(PPointer {
                    r#type: JStr::from("MAPPING"),
                    path: JStr(path),
                    source_information: c.span(m_start, c.pos() - 1),
                }));
                if !c.match_tok(T::COMMA)? {
                    break;
                }
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
        } else if key == T::CONNECTIONS && !single {
            parse_connections(c, &mut body.connections)?;
        } else if !single && conn_stores {
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                let c_start = c.pos();
                let conn = unquote_path_iae(&c.parse_qualified_name()?)?;
                let c_span = c.span(c_start, c.pos() - 1);
                c.expect(T::COLON)?;
                c.expect(T::BRACKET_OPEN)?;
                let mut stores = Vec::new();
                while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                    let p_start = c.pos();
                    // the optional (dataspace) element-type qualifier: `(Kind) path`
                    let mut ptr_type = None;
                    if c.peek() == T::PAREN_OPEN {
                        c.advance()?;
                        ptr_type = Some(JStr(to_upper_root(&c.parse_identifier()?)));
                        c.expect(T::PAREN_CLOSE)?;
                    }
                    let path = unquote_path_iae(&c.parse_qualified_name()?)?;
                    stores.push(Box::new(PStorePointer {
                        path: JStr(path),
                        source_information: c.span(p_start, c.pos() - 1),
                        r#type: ptr_type,
                    }));
                    c.match_tok(T::COMMA)?;
                }
                let group_end = c.pos();
                c.expect(T::BRACKET_CLOSE)?;
                body.connection_stores.push(Box::new(PConnectionStores {
                    connection_pointer: PConnectionValue::PConnectionPointer(Box::new(PConnectionPointer {
                        connection: JStr(conn),
                        source_information: c_span,
                    })),
                    store_pointers: stores,
                    source_information: c.span(c_start, group_end),
                }));
                c.match_tok(T::COMMA)?;
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
        } else if key == T::CONNECTION && single {
            let c_start = c.pos();
            let conn = unquote_path_iae(&c.parse_qualified_name()?)?;
            let c_span = c.span(c_start, c.pos() - 1);
            body.connection_stores.push(Box::new(PConnectionStores {
                connection_pointer: PConnectionValue::PConnectionPointer(Box::new(PConnectionPointer {
                    connection: JStr(conn),
                    source_information: c_span.clone(),
                })),
                store_pointers: Vec::new(),
                source_information: c_span,
            }));
            c.expect(T::SEMI_COLON)?;
        } else {
            let kind = if single { "SingleConnectionRuntime" } else { "Runtime" };
            return Err(c.error(jcat!("unknown key '", key_text, "' inside ", kind, " '", qn, "'")));
        }
    }
    Ok(seen_mappings)
}

/// `parseEmbeddedBody(islandText, baseLine, baseColumn, dialect)`: a service's anonymous runtime island, re-lexed with
/// the walker offsets; its span the content's (first through last token), an empty island refused.
pub fn parse_embedded_body(
    island_text: &[u16],
    base_line: i32,
    base_column: i32,
    dialect: Dialect,
) -> R<PEmbeddedRuntime> {
    let ts = tokenize(island_text);
    let mut ic = IslandCursor {
        tokens: &ts,
        pos: 0,
        dialect,
        line_offset: base_line - 1,
        col_offset: base_column - 1,
        runtime: true,
    };
    if ts.count() == 0 {
        return Err(ic.error_str("Embedded runtime must not be empty"));
    }
    let content_span = ic.span(0, ts.count() - 1);
    let mut body = Body::default();
    parse_body_keys(&mut ic, false, &crate::ustr::u("<embedded>"), None, &mut body, 0)?;
    Ok(PEmbeddedRuntime {
        mappings: body.mappings,
        connections: body.connections,
        connection_stores: body.connection_stores,
        source_information: content_span,
    })
}
