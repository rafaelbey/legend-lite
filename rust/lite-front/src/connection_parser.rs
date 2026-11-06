// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Connection` (core/'s `parser.section.ConnectionSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4c): every
//! connection spelling -- the element envelope (`<Flavor> qn { body }`) and the value body it shares with a runtime's
//! embedded island (`#{ <Flavor> {...} }#`, re-lexed with the engine's walker offsets: [`parse_island_value`]). The
//! flavors: Json/Xml model, model chain, relational database (its type, datasource specifications, auth strategies,
//! post-processors, generation configs, time zone, local mode), ServiceStore, Deephaven, MongoDB and Elasticsearch
//! (their authentication islands, re-lexed: user/password, Kerberos, API key, GCP WIF with an AWS IdP, PSK, encrypted
//! private key, and the vault secrets they hold).
//!
//! Java's answers reproduced, quirks included: an island's errors are placed in the island's own lines and columns
//! (Java's re-lex cursor shifts spans, not errors); a duplicate key is refused where each site anchors it (the cursor,
//! the `specification`/`auth` keyword, the element); a table mapper's first unknown key is the first in the order Java's
//! `HashMap` iterates its keys; the Deephaven and Elasticsearch values' spans overshoot the close by the engine's
//! re-parse offset.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::cursor::{Cursor, IslandCursor, R, once, once_at, throw_at, unquote_path_iae};
use crate::database_parser::to_upper_root;
use crate::doors::{Dialect, JavaError};
use crate::jcat;
use crate::jnet::uri_parses;
use crate::jnum::parse_long;
use crate::jtypes::{JStr, hash_map_order};
use crate::lexer::{TokenStream, TokenType, tokenize};
use crate::mapping_tests::replace_units;
use crate::nodes::Span;
use crate::records::{
    PApiKeyAuth, PApiToken, PAthenaSpec, PAuroraSpec, PAuthSpecValue, PAuthStrategy, PAwsCredentials, PAwsDefault,
    PAwsSecret, PAwsStatic, PAwsStsRole, PBigQuerySpec, PConnection, PConnectionValue, PDatabricksSpec,
    PDatasourceSpec, PDeephavenConnection, PDelegatedKerberos, PDuckDBSpec, PElasticsearchConnection, PEpkAuth,
    PExtractSubQueriesAsCtesPostProcessor, PGCPApplicationDefaultCredentials, PGcpWifAuth, PGcpWifIslandAuth,
    PGenerationFeaturesConfig, PGlobalAuroraSpec, PH2Default, PH2EmbeddedSpec, PH2Local, PJsonModelConnection,
    PKerberosAuth, PMapper, PMapperPostProcessor, PMemSqlSpec, PMiddleTierUserNamePassword, PModelChainConnection,
    PMongoAuth, PMongoDbConnection, PMongoSecret, PMongoServerUrl, POAuth, POracleSpec, PPointer, PPostProcessor,
    PPskAuth, PRedshiftSpec, PRelationalDatabaseConnection, PRelationalMapperPostProcessor, PSQLiteSpec, PSchemaMapper,
    PServiceStoreConnection, PSnowflakePublic, PSnowflakeSpec, PSpannerSpec, PStaticSpec, PTableMapper, PTestAuth,
    PTrinoKerberosAuth, PTrinoSpec, PTrinoSsl, PUserNamePassword, PVaultSecret, PXmlModelConnection, SourceInfo,
};
use crate::sections::{boolean_value, scan_island, split_last, string_value};
use crate::ustr::{ends_with, eq, index_of, strip, trim, u};
use std::collections::HashSet;

use TokenType as T;

/// `DATABASE_TYPES`: `DatabaseType` at the 4.138.2 referee pin.
const DATABASE_TYPES: &[&str] = &[
    "DB2",
    "H2",
    "MemSQL",
    "Sybase",
    "SybaseIQ",
    "Composite",
    "Postgres",
    "SqlServer",
    "Hive",
    "Snowflake",
    "Presto",
    "Trino",
    "BigQuery",
    "Redshift",
    "Databricks",
    "Spanner",
    "Athena",
    "DuckDB",
    "Oracle",
    "ClickHouse",
    "Aurora",
];

/// `LITE_DATABASE_TYPES`: the database types lite adds, declared extensions the engine's dialect refuses.
const LITE_DATABASE_TYPES: &[&str] = &["SQLite", "MariaDB", "MySQL"];

fn listed(list: &[&str], v: &[u16]) -> bool {
    list.iter().any(|s| eq(v, s))
}

fn key_name(key: &[u16]) -> String {
    String::from_utf16_lossy(key)
}

fn boxed(s: SourceInfo) -> Span {
    Some(Box::new(s))
}

fn si(start_line: i32, start_column: i32, end_line: i32, end_column: i32) -> SourceInfo {
    SourceInfo { source_id: JStr::default(), start_line, start_column, end_line, end_column }
}

// ================================================================ the element envelope

/// `ConnectionSectionGrammar.parseElement`: `<Flavor> qn { body }` -- the envelope and its value share one span.
pub fn parse_connection<'s>(c: &mut impl Cursor<'s>) -> R<PConnection> {
    let decl_start = c.pos();
    let flavor = c.safe_text();
    c.advance()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    let value = parse_value(c, &flavor, decl_start, true)?;
    Ok(PConnection { pkg: JStr(pkg), name: JStr(name), value, source_information: c.span(decl_start, c.pos() - 1) })
}

/// `parseIslandValue(islandText, baseLine, baseColumn, dialect)`: an embedded runtime island's connection, re-lexed
/// with the walker offsets (the line offset on every line, the column offset on the first only).
pub fn parse_island_value(
    island_text: &[u16],
    base_line: i32,
    base_column: i32,
    dialect: Dialect,
) -> R<PConnectionValue> {
    let ts = tokenize(island_text);
    let mut c = IslandCursor {
        tokens: &ts,
        pos: 0,
        dialect,
        line_offset: base_line - 1,
        col_offset: base_column - 1,
        runtime: false,
    };
    let flavor = c.safe_text();
    let f_start = c.pos;
    c.advance()?;
    parse_value(&mut c, &flavor, f_start, false)
}

/// `parseValue(c, flavor, declStart, standalone)`: one connection value body for `flavor` (`standalone`: a section
/// element's model connection names its `ModelStore`, an embedded one none).
pub fn parse_value<'s>(
    c: &mut impl Cursor<'s>,
    flavor: &[u16],
    decl_start: usize,
    standalone: bool,
) -> R<PConnectionValue> {
    let element = || standalone.then(|| JStr::from("ModelStore"));
    Ok(match key_name(flavor).as_str() {
        f @ ("JsonModelConnection" | "XmlModelConnection") => {
            let (class_name, class_span, url) = parse_model_connection_body(c, decl_start)?;
            let span = c.span(decl_start, c.pos() - 1);
            if f == "JsonModelConnection" {
                PConnectionValue::PJsonModelConnection(Box::new(PJsonModelConnection {
                    class_name,
                    class_source_information: boxed(class_span),
                    element: element(),
                    url,
                    source_information: span,
                }))
            } else {
                PConnectionValue::PXmlModelConnection(Box::new(PXmlModelConnection {
                    class_name,
                    class_source_information: boxed(class_span),
                    element: element(),
                    url,
                    source_information: span,
                }))
            }
        }
        "ModelChainConnection" => {
            PConnectionValue::PModelChainConnection(Box::new(parse_model_chain_body(c, decl_start, standalone)?))
        }
        "RelationalDatabaseConnection" => {
            PConnectionValue::PRelationalDatabaseConnection(Box::new(parse_relational_connection_body(c, decl_start)?))
        }
        "ServiceStoreConnection" => {
            PConnectionValue::PServiceStoreConnection(Box::new(parse_service_store_connection_body(c, decl_start)?))
        }
        "DeephavenConnection" => PConnectionValue::PDeephavenConnection(Box::new(parse_deephaven_connection_body(c)?)),
        "MongoDBConnection" => {
            PConnectionValue::PMongoDbConnection(Box::new(parse_mongo_connection_body(c, decl_start)?))
        }
        "Elasticsearch7ClusterConnection" => {
            PConnectionValue::PElasticsearchConnection(Box::new(parse_elasticsearch_body(c)?))
        }
        _ => {
            // anchored at the element (the engine's walker: the connection ctx's source information)
            let at = if c.peek() == T::BRACE_OPEN { c.pos() + 1 } else { c.pos() };
            return Err(throw_at(c.tokens(), at, jcat!("unsupported connection flavor: ", flavor)));
        }
    })
}

/// A STRING's decoded body where Java reads `c.text()` before `expect(STRING)` (the text `""` past the end).
fn quoted_string<'s>(c: &mut impl Cursor<'s>) -> R<JStr> {
    let quoted = c.slack_text();
    c.expect(T::STRING)?;
    Ok(JStr(c.unquote_and_unescape(quoted)?))
}

fn string<'s>(c: &mut impl Cursor<'s>) -> R<JStr> {
    Ok(JStr(string_value(c)?))
}

/// `parseModelConnectionBody`: `{ class: qn; url: '...'; }`, both required.
fn parse_model_connection_body<'s>(c: &mut impl Cursor<'s>, decl_start: usize) -> R<(JStr, SourceInfo, JStr)> {
    c.expect(T::BRACE_OPEN)?;
    let mut cls = None;
    let mut url = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), decl_start)?;
        c.expect(T::COLON)?;
        if eq(&key, "class") {
            let c_s = c.pos();
            let path = unquote_path_iae(&c.parse_qualified_name()?)?;
            cls = Some((JStr(path), c.span_of(c_s, c.pos() - 1)));
        } else if eq(&key, "url") {
            url = Some(quoted_string(c)?);
        } else {
            return Err(c.error(jcat!("unknown model-connection key: ", key)));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    match (cls, url) {
        (Some((cls, span)), Some(url)) => Ok((cls, span, url)),
        _ => Err(throw_at(c.tokens(), decl_start, JStr::from("model connection needs class and url"))),
    }
}

/// `parseModelChainBody`: `{ mappings: [qn, ...]; }`, its mappings span the key through the `;`.
fn parse_model_chain_body<'s>(
    c: &mut impl Cursor<'s>,
    decl_start: usize,
    standalone: bool,
) -> R<PModelChainConnection> {
    c.expect(T::BRACE_OPEN)?;
    let mut mappings = Vec::new();
    let mut map_span: Option<SourceInfo> = None;
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let k_s = c.pos();
        let key = c.safe_text();
        if c.peek() != T::MAPPINGS {
            return Err(c.error(jcat!("unknown ModelChainConnection key: ", key)));
        }
        if map_span.is_some() {
            return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'mappings' should be specified only once")));
        }
        c.advance()?;
        c.expect(T::COLON)?;
        c.expect(T::BRACKET_OPEN)?;
        while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
            mappings.push(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            c.match_tok(T::COMMA)?;
        }
        c.expect(T::BRACKET_CLOSE)?;
        c.match_tok(T::SEMI_COLON)?;
        map_span = Some(c.span_of(k_s, c.pos() - 1));
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(map_span) = map_span else {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("ModelChainConnection needs mappings")));
    };
    Ok(PModelChainConnection {
        element: standalone.then(|| JStr::from("ModelStore")),
        mappings,
        mappings_source_information: boxed(map_span),
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `parseServiceStoreConnectionBody`: `{ store: qn; baseUrl: 'url'; }`.
fn parse_service_store_connection_body<'s>(c: &mut impl Cursor<'s>, decl_start: usize) -> R<PServiceStoreConnection> {
    c.expect(T::BRACE_OPEN)?;
    let mut element = None;
    let mut element_span = None;
    let mut base_url = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if eq(&key, "store") {
            let e_s = c.pos();
            element = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            element_span = c.span(e_s, c.pos() - 1);
        } else if eq(&key, "baseUrl") {
            let v = string_value(c)?;
            if ends_with(&v, "/") {
                return Err(c.error_str("baseUrl should not end with '/'"));
            }
            base_url = Some(JStr(v));
        } else {
            return Err(c.error(jcat!("unknown ServiceStoreConnection key: ", key)));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(base_url) = base_url else {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("ServiceStoreConnection needs baseUrl")));
    };
    Ok(PServiceStoreConnection {
        base_url,
        element,
        element_source_information: element_span,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `islandKind`: the kind between the opener's `#` and `{` (`# PSK {` is `PSK`), trimmed; not consumed.
fn island_kind<'s>(c: &impl Cursor<'s>) -> Vec<u16> {
    let t = c.text();
    let brace = index_of(t, u16::from(b'{'), 0).unwrap_or(t.len());
    trim(&t[1..brace]).to_vec()
}

/// `reLexIsland`'s product: the island's content text (the caller lexes it again), the walker offsets of its first
/// token in the host's stream (its own lines and columns, re-lexed or not), and the closer's token.
struct Relexed {
    text: Vec<u16>,
    line_offset: i32,
    col_offset: i32,
    end_tok: usize,
}

impl Relexed {
    fn cursor<'t, 'a>(&self, ts: &'t TokenStream<'a>, dialect: Dialect) -> IslandCursor<'t, 'a> {
        IslandCursor {
            tokens: ts,
            pos: 0,
            dialect,
            line_offset: self.line_offset,
            col_offset: self.col_offset,
            runtime: false,
        }
    }
}

/// `reLexIsland`: the island at the cursor consumed through its `}#`.
fn re_lex_island<'s>(c: &mut impl Cursor<'s>) -> R<Relexed> {
    let (emb_start, end) = scan_island(c)?;
    let text = c.reconstruct_text(emb_start, end);
    let end_tok = c.pos();
    c.expect(T::ISLAND_END)?;
    let ts = c.tokens();
    Ok(Relexed {
        text,
        line_offset: ts.start_line(emb_start) as i32 - 1,
        col_offset: ts.start_column(emb_start) as i32 - 1,
        end_tok,
    })
}

/// `parseDeephavenConnectionBody`: `{ store: qn; serverUrl: 'url' authentication: # PSK { psk: 'v'; }#; }` -- the
/// server url takes no `;`; the value's span the first body token through the close, its end column the close's
/// own (unshifted) plus 4.
fn parse_deephaven_connection_body<'s>(c: &mut impl Cursor<'s>) -> R<PDeephavenConnection> {
    c.expect(T::BRACE_OPEN)?;
    let body_start = c.pos();
    let mut element = None;
    let mut element_span = None;
    let mut server_url = None;
    let mut psk = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        match key_name(&key).as_str() {
            "store" => {
                let e_s = c.pos();
                element = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
                element_span = c.span(e_s, c.pos() - 1);
                c.expect(T::SEMI_COLON)?;
            }
            "serverUrl" => {
                server_url = Some(string(c)?);
                if c.peek() == T::SEMI_COLON {
                    return Err(c.error_str("Unexpected token"));
                }
            }
            "authentication" => {
                if c.peek() != T::ISLAND_OPEN {
                    return Err(c.error_str("DeephavenConnection authentication must be a # PSK {...}# island"));
                }
                let kind = island_kind(c);
                if !eq(&kind, "PSK") {
                    return Err(c.error(jcat!("unsupported Deephaven auth kind: ", kind)));
                }
                let ip = re_lex_island(c)?;
                let its = tokenize(&ip.text);
                let mut ic = ip.cursor(&its, c.dialect());
                let ik = ic.parse_identifier()?;
                ic.expect(T::COLON)?;
                if !eq(&ik, "psk") {
                    return Err(ic.error(jcat!("unknown PSK key: ", ik)));
                }
                psk = Some(string(&mut ic)?);
                ic.expect(T::SEMI_COLON)?;
                // the rule has no EOF: trailing island tokens are ignored
                while !ic.at_end() {
                    ic.advance()?;
                }
                c.expect(T::SEMI_COLON)?;
            }
            _ => return Err(c.error(jcat!("unknown DeephavenConnection key: ", key))),
        }
    }
    let close_tok = c.pos();
    c.expect(T::BRACE_CLOSE)?;
    let (Some(server_url), Some(psk)) = (server_url, psk) else {
        return Err(c.error_str("DeephavenConnection needs serverUrl and authentication"));
    };
    let sp = c.span_of(body_start, close_tok);
    let v_span = si(sp.start_line, sp.start_column, sp.end_line, c.tokens().start_column(close_tok) as i32 + 4);
    Ok(PDeephavenConnection {
        server_url,
        psk,
        element,
        element_source_information: element_span,
        source_information: boxed(v_span),
    })
}

/// `parseMongoConnectionBody`: `{ database: id; store: qn; serverURLs: [host:port, ...]; authentication: # Kind
/// {...}#; }`, every field `;`-terminated, the urls required.
fn parse_mongo_connection_body<'s>(c: &mut impl Cursor<'s>, decl_start: usize) -> R<PMongoDbConnection> {
    c.expect(T::BRACE_OPEN)?;
    let mut database = None;
    let mut element: Option<Vec<u16>> = None;
    let mut element_span = None;
    let mut urls = Vec::new();
    let mut auth = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_tok = c.pos();
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        match key_name(&key).as_str() {
            "database" => database = Some(JStr(c.parse_identifier()?)),
            "store" => {
                let e_s = c.pos();
                let mut e = unquote_path_iae(&c.parse_qualified_name()?)?;
                // mixed `.` and `::` separators are the engine's
                while c.peek() == T::DOT {
                    c.advance()?;
                    e.push(u16::from(b'.'));
                    e.extend(unquote_path_iae(&c.parse_qualified_name()?)?);
                }
                element = Some(e);
                element_span = c.span(e_s, c.pos() - 1);
            }
            "serverURLs" => {
                c.expect(T::BRACKET_OPEN)?;
                while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                    // a host is the raw source up to its ':' (dashes and dots ride unquoted)
                    let h_s = c.pos();
                    while !c.at_end() && c.peek() != T::COLON {
                        c.advance()?;
                    }
                    let host = c.reconstruct_text(h_s, c.pos());
                    c.expect(T::COLON)?;
                    let port = c.consume_long()?;
                    urls.push(Box::new(PMongoServerUrl { base_url: JStr(host), port }));
                    c.match_tok(T::COMMA)?;
                }
                c.expect(T::BRACKET_CLOSE)?;
            }
            "authentication" => auth = Some(parse_user_password_auth_island(c, key_tok)?),
            _ => return Err(c.error(jcat!("unknown MongoDBConnection key: ", key))),
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let (Some(database_name), Some(auth)) = (database, auth) else {
        return Err(c.error_str("MongoDBConnection needs database and authentication"));
    };
    if urls.is_empty() {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("Field 'serverURLs' is required")));
    }
    Ok(PMongoDbConnection {
        database_name,
        server_urls: urls,
        auth,
        element: element.map(JStr),
        element_source_information: element_span,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

// ================================================================ the relational database connection

#[derive(Default)]
struct RelationalBody {
    element: Option<Vec<u16>>,
    element_span: Span,
    db_type: Option<JStr>,
    spec: Option<PDatasourceSpec>,
    auth: Option<PAuthStrategy>,
    posts: Vec<PPostProcessor>,
    quote_identifiers: Option<bool>,
    query_time_out: Option<i64>,
    query_gen_configs: Option<Vec<Box<PGenerationFeaturesConfig>>>,
    time_zone: Option<JStr>,
    local_mode: bool,
}

/// `parseRelationalConnectionBody`: `{ store; type; specification; auth; postProcessors; quoteIdentifiers;
/// queryTimeOutInSeconds; queryGenerationConfigs; timezone; mode; }`, a duplicate refused at the element; `mode: local`
/// with a Snowflake store synthesizes the specification and auth from the store's path.
fn parse_relational_connection_body<'s>(
    c: &mut impl Cursor<'s>,
    decl_start: usize,
) -> R<PRelationalDatabaseConnection> {
    c.expect(T::BRACE_OPEN)?;
    let mut b = RelationalBody::default();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let k_s = c.pos();
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), decl_start)?;
        c.expect(T::COLON)?;
        relational_key(c, &key, k_s, &mut b)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    if b.local_mode
        && b.db_type.as_ref().is_some_and(|t| t == "Snowflake")
        && let Some(element) = &b.element
    {
        // the snowflake extension's local mode: spec and auth from the store path (note `publicuserName`)
        let tail = JStr(replace_units(element, &u("::"), &u("-")));
        let local = |field: &str| jcat!("legend-local-snowflake-", field, "-", tail);
        b.spec = Some(PDatasourceSpec::PSnowflakeSpec(Box::new(PSnowflakeSpec {
            account_name: local("accountName"),
            account_type: None,
            cloud_type: Some(local("cloudType")),
            database_name: local("databaseName"),
            enable_query_tags: None,
            non_proxy_hosts: None,
            organization: None,
            proxy_host: None,
            proxy_port: None,
            quoted_identifiers_ignore_case: None,
            region: local("region"),
            role: Some(local("role")),
            temp_table_db: None,
            temp_table_schema: None,
            warehouse_name: local("warehouseName"),
            source_information: None,
        })));
        b.auth = Some(PAuthStrategy::PSnowflakePublic(Box::new(PSnowflakePublic {
            pass_phrase_vault_reference: local("passphraseVaultReference"),
            private_key_vault_reference: local("privateKeyVaultReference"),
            public_user_name: local("publicuserName"),
            source_information: None,
        })));
    }
    let (Some(db_type), Some(spec)) = (b.db_type, b.spec) else {
        return Err(throw_at(
            c.tokens(),
            decl_start,
            JStr::from("RelationalDatabaseConnection needs type and specification"),
        ));
    };
    let Some(auth) = b.auth else {
        return Err(throw_at(c.tokens(), decl_start, JStr::from("RelationalDatabaseConnection needs auth")));
    };
    Ok(PRelationalDatabaseConnection {
        authentication_strategy: auth,
        database_type: db_type,
        datasource_specification: spec,
        element: b.element.map(JStr),
        element_source_information: b.element_span,
        local_mode: b.local_mode.then_some(true),
        post_processors: b.posts,
        query_generation_configs: b.query_gen_configs,
        query_time_out_in_seconds: b.query_time_out,
        quote_identifiers: b.quote_identifiers,
        time_zone: b.time_zone,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// One `key: value` of a relational database connection's body.
fn relational_key<'s>(c: &mut impl Cursor<'s>, key: &[u16], k_s: usize, b: &mut RelationalBody) -> R<()> {
    match key_name(key).as_str() {
        "store" => {
            let e_s = c.pos();
            b.element = Some(unquote_path_iae(&c.parse_qualified_name()?)?);
            b.element_span = c.span(e_s, c.pos() - 1);
            c.expect(T::SEMI_COLON)?;
        }
        "type" => {
            let t_tok = c.pos();
            let db_type = c.parse_identifier()?;
            if !listed(DATABASE_TYPES, &db_type)
                && !(!c.dialect().refuses_lite_extensions() && listed(LITE_DATABASE_TYPES, &db_type))
            {
                return Err(throw_at(c.tokens(), t_tok, jcat!("Unknown database type '", db_type, "'")));
            }
            b.db_type = Some(JStr(db_type));
            c.expect(T::SEMI_COLON)?;
        }
        "specification" => b.spec = Some(parse_datasource_spec(c, k_s)?),
        "auth" => b.auth = Some(parse_auth_strategy(c, k_s)?),
        "postProcessors" => parse_mapper_post_processors(c, &mut b.posts)?,
        "quoteIdentifiers" => {
            b.quote_identifiers = Some(boolean_value(c)?);
            c.expect(T::SEMI_COLON)?;
        }
        "queryTimeOutInSeconds" => {
            b.query_time_out = Some(c.consume_long()?);
            c.expect(T::SEMI_COLON)?;
        }
        "queryGenerationConfigs" => {
            let mut configs = Vec::new();
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                configs.push(Box::new(parse_generation_features_config(c)?));
                c.match_tok(T::COMMA)?;
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
            b.query_gen_configs = Some(configs);
        }
        "timezone" => {
            if c.peek() == T::STRING {
                // a quoted zone id: the quotes are syntax, stripped
                let v = c.unquote_and_unescape(c.text())?;
                c.advance()?;
                b.time_zone = Some(JStr(v));
            } else {
                // an unquoted offset: a sign and digits
                let mut z = Vec::new();
                if c.peek() == T::PLUS {
                    z.push(u16::from(b'+'));
                    c.advance()?;
                } else if c.peek() == T::MINUS {
                    z.push(u16::from(b'-'));
                    c.advance()?;
                }
                z.extend_from_slice(c.slack_text());
                c.expect(T::INTEGER)?;
                b.time_zone = Some(JStr(z));
            }
            c.expect(T::SEMI_COLON)?;
        }
        "mode" => {
            let m = c.parse_identifier()?;
            if !eq(&m, "local") {
                return Err(c.error(jcat!("unknown connection mode: ", m)));
            }
            b.local_mode = true;
            c.expect(T::SEMI_COLON)?;
        }
        _ => return Err(c.error(jcat!("unknown RelationalDatabaseConnection key: ", key))),
    }
    Ok(())
}

/// One `GenerationFeaturesConfig { enabled: ['..']; disabled: ['..']; }`, a duplicate refused at its kind.
fn parse_generation_features_config<'s>(c: &mut impl Cursor<'s>) -> R<PGenerationFeaturesConfig> {
    let g_s = c.pos();
    let g_kind = c.parse_identifier()?;
    if !eq(&g_kind, "GenerationFeaturesConfig") {
        return Err(c.error(jcat!("unknown query generation config: ", g_kind)));
    }
    c.expect(T::BRACE_OPEN)?;
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while c.peek() != T::BRACE_CLOSE && !c.at_end() {
        let gk = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(gk.clone()) {
            return Err(throw_at(c.tokens(), g_s, jcat!("Field '", gk, "' should be specified only once")));
        }
        let into = if eq(&gk, "enabled") {
            &mut enabled
        } else if eq(&gk, "disabled") {
            &mut disabled
        } else {
            return Err(c.error(jcat!("unknown GenerationFeaturesConfig key: ", gk)));
        };
        c.expect(T::BRACKET_OPEN)?;
        while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
            into.push(string(c)?);
            c.match_tok(T::COMMA)?;
        }
        c.expect(T::BRACKET_CLOSE)?;
        c.expect(T::SEMI_COLON)?;
    }
    let g_e = c.pos();
    c.expect(T::BRACE_CLOSE)?;
    Ok(PGenerationFeaturesConfig { enabled, disabled, source_information: c.span(g_s, g_e) })
}

/// `stringOrInt`: an unquoted integer's text, or a string's body (`proxyPort`).
fn string_or_int<'s>(c: &mut impl Cursor<'s>) -> R<JStr> {
    if c.peek() == T::INTEGER {
        let v = JStr::from(c.text());
        c.advance()?;
        return Ok(v);
    }
    string(c)
}

/// `parseMapperPostProcessors`: `[ mapper { mappers: [ table {...}, schema {...} ]; }, relationalMapper { qn, ... },
/// ExtractSubQueriesAsCTEsPostProcessor, ... ];`.
fn parse_mapper_post_processors<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<PPostProcessor>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let kind_tok = c.pos();
        let kind = c.parse_identifier()?;
        if eq(&kind, "relationalMapper") {
            // the engine re-parses the keyword and the brace's content with the whitespace between them dropped,
            // offset by the keyword: a content line sits one line higher per line break before the '{'
            let brace_tok = c.pos();
            c.expect(T::BRACE_OPEN)?;
            let shift = c.span_of(brace_tok, brace_tok).start_line - c.span_of(kind_tok, kind_tok).start_line;
            let mut ptrs = Vec::new();
            while !c.at_end() && c.peek() != T::BRACE_CLOSE {
                let at = c.pos();
                let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
                let real = c.span_of(at, c.pos() - 1);
                ptrs.push(Box::new(PPointer {
                    r#type: JStr::from("QUERYPOSTPROCESSOR"),
                    path: JStr(qn),
                    source_information: boxed(SourceInfo {
                        source_id: real.source_id,
                        start_line: real.start_line - shift,
                        start_column: real.start_column,
                        end_line: real.end_line - shift,
                        end_column: real.end_column,
                    }),
                }));
                c.match_tok(T::COMMA)?;
            }
            c.expect(T::BRACE_CLOSE)?;
            out.push(PPostProcessor::PRelationalMapperPostProcessor(Box::new(PRelationalMapperPostProcessor {
                relational_mappers: ptrs,
            })));
            c.match_tok(T::COMMA)?;
            continue;
        }
        if eq(&kind, "ExtractSubQueriesAsCTEsPostProcessor") {
            out.push(PPostProcessor::PExtractSubQueriesAsCtesPostProcessor(Box::new(
                PExtractSubQueriesAsCtesPostProcessor {},
            )));
            c.match_tok(T::COMMA)?;
            continue;
        }
        if !eq(&kind, "mapper") {
            return Err(c.error(jcat!("unsupported postProcessor flavor: ", kind)));
        }
        c.expect(T::BRACE_OPEN)?;
        let mut mappers = Vec::new();
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key = c.parse_identifier()?;
            once(&mut seen, &key, c)?;
            c.expect(T::COLON)?;
            if !eq(&key, "mappers") {
                return Err(c.error(jcat!("unknown mapper key: ", key)));
            }
            c.expect(T::BRACKET_OPEN)?;
            while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                mappers.push(parse_one_mapper(c)?);
                c.match_tok(T::COMMA)?;
            }
            c.expect(T::BRACKET_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
        }
        c.expect(T::BRACE_CLOSE)?;
        out.push(PPostProcessor::PMapperPostProcessor(Box::new(PMapperPostProcessor { mappers })));
        c.match_tok(T::COMMA)?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `parseOneMapper`: `table { from; to; schemaFrom; schemaTo; }` or `schema { from; to; }` -- an unknown key refused
/// in the order Java's `HashMap` iterates the keys, after the whole body is read.
fn parse_one_mapper<'s>(c: &mut impl Cursor<'s>) -> R<PMapper> {
    let flavor = c.parse_identifier()?;
    c.expect(T::BRACE_OPEN)?;
    let mut keys: Vec<Vec<u16>> = Vec::new();
    let mut values: Vec<Vec<u16>> = Vec::new();
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        let v = quoted_string(c)?;
        keys.push(key);
        values.push(v.0);
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let table = eq(&flavor, "table");
    let allowed: &[&str] = if table { &["from", "to", "schemaFrom", "schemaTo"] } else { &["from", "to"] };
    for i in hash_map_order(&keys) {
        if !listed(allowed, &keys[i]) {
            return Err(c.error(jcat!("unknown ", flavor, " mapper key: ", keys[i])));
        }
    }
    let get = |k: &str| keys.iter().position(|x| eq(x, k)).map(|i| JStr(values[i].clone()));
    let (Some(from), Some(to)) = (get("from"), get("to")) else {
        return Err(c.error_str("mapper needs from and to"));
    };
    if table {
        let (Some(schema_from), Some(schema_to)) = (get("schemaFrom"), get("schemaTo")) else {
            return Err(c.error_str("table mapper needs schemaFrom and schemaTo"));
        };
        return Ok(PMapper::PTableMapper(Box::new(PTableMapper { from, to, schema_from, schema_to })));
    }
    if eq(&flavor, "schema") {
        return Ok(PMapper::PSchemaMapper(Box::new(PSchemaMapper { from, to })));
    }
    Err(c.error(jcat!("unsupported mapper flavor: ", flavor)))
}

// ================================================================ datasource specifications

/// `parseDatasourceSpec(c, keywordTok)`: the kind and its body; a spec's span the `specification` keyword through the
/// body's `;`.
fn parse_datasource_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let kind = c.parse_identifier()?;
    Ok(match key_name(&kind).as_str() {
        "LocalH2" => {
            let mut sqls = None;
            let mut csv = None;
            c.expect(T::BRACE_OPEN)?;
            let mut seen: HashSet<Vec<u16>> = HashSet::new();
            while !c.at_end() && c.peek() != T::BRACE_CLOSE {
                let key = c.parse_identifier()?;
                once_at(&mut seen, &key, c.tokens(), keyword_tok)?;
                c.expect(T::COLON)?;
                if eq(&key, "testDataSetupSqls") {
                    let mut list = Vec::new();
                    c.expect(T::BRACKET_OPEN)?;
                    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
                        list.push(quoted_string(c)?);
                        c.match_tok(T::COMMA)?;
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                    sqls = Some(list);
                } else if eq(&key, "testDataSetupCSV") {
                    csv = Some(quoted_string(c)?);
                } else {
                    return Err(c.error(jcat!("unknown LocalH2 key: ", key)));
                }
                c.match_tok(T::SEMI_COLON)?;
            }
            c.expect(T::BRACE_CLOSE)?;
            c.expect(T::SEMI_COLON)?;
            PDatasourceSpec::PH2Local(Box::new(PH2Local {
                test_data_setup_csv: csv,
                test_data_setup_sqls: sqls,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "Static" => parse_static_spec(c, keyword_tok)?,
        "DuckDB" => {
            let path = parse_path_spec(c, "DuckDB")?;
            PDatasourceSpec::PDuckDBSpec(Box::new(PDuckDBSpec {
                path,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "SQLite" => {
            if c.dialect().refuses_lite_extensions() {
                return Err(c.error_str("unsupported datasource specification: SQLite"));
            }
            let path = parse_path_spec(c, "SQLite")?;
            PDatasourceSpec::PSQLiteSpec(Box::new(PSQLiteSpec {
                path,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "Snowflake" => parse_snowflake_spec(c, keyword_tok)?,
        "Redshift" => parse_redshift_spec(c, keyword_tok)?,
        "Trino" => parse_trino_spec(c, keyword_tok)?,
        "Athena" => parse_athena_spec(c, keyword_tok)?,
        "Aurora" => parse_aurora_spec(c, keyword_tok)?,
        "GlobalAurora" => parse_global_aurora_spec(c, keyword_tok)?,
        "MemSql" => parse_mem_sql_spec(c, keyword_tok)?,
        "Oracle" => parse_oracle_spec(c, keyword_tok)?,
        "Spanner" => parse_spanner_spec(c, keyword_tok)?,
        "Databricks" => parse_databricks_spec(c, keyword_tok)?,
        "BigQuery" => parse_big_query_spec(c, keyword_tok)?,
        "EmbeddedH2" => parse_embedded_h2_spec(c, keyword_tok)?,
        _ => {
            return Err(c.error(jcat!(
                "unsupported datasource specification: ",
                kind,
                " (corpus-censused shapes only)"
            )));
        }
    })
}

/// `DuckDB { (path: '...';)* };` and lite's `SQLite`, the same shape: the duplicate refused at the cursor.
fn parse_path_spec<'s>(c: &mut impl Cursor<'s>, kind: &str) -> R<Option<JStr>> {
    c.expect(T::BRACE_OPEN)?;
    let mut path = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        once(&mut seen, &key, c)?;
        c.expect(T::COLON)?;
        if !eq(&key, "path") {
            return Err(c.error(jcat!("unknown ", kind, " key: ", key)));
        }
        path = Some(quoted_string(c)?);
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    c.expect(T::SEMI_COLON)?;
    Ok(path)
}

/// A spec body's key loop: `{ key: value; ... }` then `;`, each key once (at the cursor, or at `anchor`), each value
/// read by `value`; the closing `};` taken.
fn spec_body<'s, C: Cursor<'s>>(
    c: &mut C,
    anchor: Option<usize>,
    mut value: impl FnMut(&mut C, &str, &[u16]) -> R<()>,
) -> R<()> {
    c.expect(T::BRACE_OPEN)?;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        match anchor {
            Some(a) => once_at(&mut seen, &key, c.tokens(), a)?,
            None => once(&mut seen, &key, c)?,
        }
        c.expect(T::COLON)?;
        value(c, &key_name(&key), &key)?;
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    c.expect(T::SEMI_COLON)
}

/// `parseStaticSpec`: `Static { name; host; port; };` -- `name` the database's name; the missing port refused at the
/// keyword.
fn parse_static_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut name, mut host, mut port) = (None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "name" => name = Some(quoted_string(c)?),
            "host" => host = Some(quoted_string(c)?),
            "port" => port = Some(c.consume_long()?),
            _ => return Err(c.error(jcat!("unknown Static key: ", key))),
        }
        Ok(())
    })?;
    let (Some(database_name), Some(host)) = (name, host) else {
        return Err(c.error_str("Static needs name (or database) and host"));
    };
    let Some(port) = port else {
        return Err(throw_at(c.tokens(), keyword_tok, JStr::from("Field 'port' is required")));
    };
    Ok(PDatasourceSpec::PStaticSpec(Box::new(PStaticSpec {
        database_name,
        host,
        port,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `parseSnowflakeSpec`: `Snowflake { name; account; warehouse; region; ... };`.
fn parse_snowflake_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let mut s = PSnowflakeSpec {
        account_name: JStr::default(),
        account_type: None,
        cloud_type: None,
        database_name: JStr::default(),
        enable_query_tags: None,
        non_proxy_hosts: None,
        organization: None,
        proxy_host: None,
        proxy_port: None,
        quoted_identifiers_ignore_case: None,
        region: JStr::default(),
        role: None,
        temp_table_db: None,
        temp_table_schema: None,
        warehouse_name: JStr::default(),
        source_information: None,
    };
    let (mut name, mut account, mut warehouse, mut region) = (None, None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "name" => name = Some(string(c)?),
            "account" => account = Some(string(c)?),
            "warehouse" => warehouse = Some(string(c)?),
            "region" => region = Some(string(c)?),
            // a bare enum identifier (VPS / MultiTenant)
            "accountType" => s.account_type = Some(JStr(c.parse_identifier()?)),
            "cloudType" => s.cloud_type = Some(string(c)?),
            "enableQueryTags" => s.enable_query_tags = Some(boolean_value(c)?),
            "organization" => s.organization = Some(string(c)?),
            "role" => s.role = Some(string(c)?),
            "proxyHost" => s.proxy_host = Some(string(c)?),
            "proxyPort" => s.proxy_port = Some(string_or_int(c)?),
            "nonProxyHosts" => s.non_proxy_hosts = Some(string(c)?),
            "tempTableDb" => s.temp_table_db = Some(string(c)?),
            "tempTableSchema" => s.temp_table_schema = Some(string(c)?),
            "quotedIdentifiersIgnoreCase" => s.quoted_identifiers_ignore_case = Some(boolean_value(c)?),
            _ => return Err(c.error(jcat!("unknown Snowflake key: ", key))),
        }
        Ok(())
    })?;
    let (Some(name), Some(account), Some(warehouse), Some(region)) = (name, account, warehouse, region) else {
        return Err(c.error_str("Snowflake needs name, account, warehouse and region"));
    };
    s.database_name = name;
    s.account_name = account;
    s.warehouse_name = warehouse;
    s.region = region;
    s.source_information = c.span(keyword_tok, c.pos() - 1);
    Ok(PDatasourceSpec::PSnowflakeSpec(Box::new(s)))
}

/// `parseRedshiftSpec`: `Redshift { name; host; port; clusterID; region; endpointURL?; };` -- no span on the wire.
fn parse_redshift_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut name, mut host, mut port, mut cluster_id, mut region, mut endpoint_url) =
        (None, None, None, None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "name" => name = Some(string(c)?),
            "host" => host = Some(string(c)?),
            "port" => port = Some(c.consume_long()?),
            "clusterID" => cluster_id = Some(string(c)?),
            "region" => region = Some(string(c)?),
            "endpointURL" => endpoint_url = Some(string(c)?),
            _ => return Err(c.error(jcat!("unknown Redshift key: ", key))),
        }
        Ok(())
    })?;
    let (Some(database_name), Some(host), Some(port), Some(cluster_id), Some(region)) =
        (name, host, port, cluster_id, region)
    else {
        return Err(c.error_str("Redshift needs name, host, port, clusterID and region"));
    };
    Ok(PDatasourceSpec::PRedshiftSpec(Box::new(PRedshiftSpec {
        cluster_id,
        database_name,
        endpoint_url,
        host,
        port,
        region,
        source_information: None,
    })))
}

/// `parseAthenaSpec`: `Athena { region; database?; workGroup?; outputLocation?; catalog?; athenaEndpoint?; };`.
fn parse_athena_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let mut a = PAthenaSpec {
        athena_endpoint: None,
        catalog: None,
        database: None,
        output_location: None,
        region: JStr::default(),
        work_group: None,
        source_information: None,
    };
    let mut region = None;
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "region" => region = Some(string(c)?),
            "database" => a.database = Some(string(c)?),
            "workGroup" => a.work_group = Some(string(c)?),
            "outputLocation" => a.output_location = Some(string(c)?),
            "catalog" => a.catalog = Some(string(c)?),
            "athenaEndpoint" => a.athena_endpoint = Some(string(c)?),
            _ => return Err(c.error(jcat!("unknown Athena key: ", key))),
        }
        Ok(())
    })?;
    let Some(region) = region else {
        return Err(c.error_str("Athena needs region"));
    };
    a.region = region;
    a.source_information = c.span(keyword_tok, c.pos() - 1);
    Ok(PDatasourceSpec::PAthenaSpec(Box::new(a)))
}

/// `parseAuroraSpec`: `Aurora { host; port; name; clusterInstanceHostPattern?; };`.
fn parse_aurora_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut host, mut port, mut name, mut pattern) = (None, None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "host" => host = Some(string(c)?),
            "port" => port = Some(c.consume_long()?),
            "name" => name = Some(string(c)?),
            "clusterInstanceHostPattern" => pattern = Some(string(c)?),
            _ => return Err(c.error(jcat!("unknown Aurora key: ", key))),
        }
        Ok(())
    })?;
    let (Some(host), Some(port), Some(name)) = (host, port, name) else {
        return Err(c.error_str("Aurora needs host, port and name"));
    };
    Ok(PDatasourceSpec::PAuroraSpec(Box::new(PAuroraSpec {
        cluster_instance_host_pattern: pattern,
        host,
        name,
        port,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `parseGlobalAuroraSpec`: `GlobalAurora { host; port; name; region; globalClusterInstanceHostPatterns: [...]; };`,
/// all five required.
fn parse_global_aurora_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut host, mut port, mut name, mut region, mut patterns) = (None, None, None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "host" => host = Some(string(c)?),
            "port" => port = Some(c.consume_long()?),
            "name" => name = Some(string(c)?),
            "region" => region = Some(string(c)?),
            "globalClusterInstanceHostPatterns" => {
                let mut list = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
                    list.push(string(c)?);
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                patterns = Some(list);
            }
            _ => return Err(c.error(jcat!("unknown GlobalAurora key: ", key))),
        }
        Ok(())
    })?;
    let (Some(host), Some(port), Some(name), Some(region), Some(patterns)) = (host, port, name, region, patterns)
    else {
        return Err(c.error_str("GlobalAurora needs host, port, name, region and globalClusterInstanceHostPatterns"));
    };
    Ok(PDatasourceSpec::PGlobalAuroraSpec(Box::new(PGlobalAuroraSpec {
        global_cluster_instance_host_patterns: patterns,
        host,
        name,
        port,
        region,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `Boolean.valueOf(s)`: `true` in any case is true, anything else false.
fn java_boolean_value_of(s: &[u16]) -> bool {
    s.len() == 4 && String::from_utf16_lossy(s).eq_ignore_ascii_case("true")
}

/// `parseMemSqlSpec`: `MemSql { host; port: '3306'; databaseName?; useSsl: 'true'?; };` -- port and useSsl strings,
/// converted as the engine's extension converts them (`Long.valueOf`, `Boolean.valueOf`, each stripped).
fn parse_mem_sql_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut host, mut port, mut database_name, mut use_ssl) = (None, None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "host" => host = Some(string(c)?),
            "port" => {
                let port_tok = c.pos();
                let text = string_value(c)?;
                match parse_long(strip(&text)) {
                    Ok(v) => port = Some(v),
                    Err(_) => {
                        return Err(throw_at(
                            c.tokens(),
                            port_tok,
                            jcat!("MemSql port must be an integer, got '", text, "'"),
                        ));
                    }
                }
            }
            "databaseName" => database_name = Some(string(c)?),
            "useSsl" => use_ssl = Some(java_boolean_value_of(strip(&string_value(c)?))),
            _ => return Err(c.error(jcat!("unknown MemSql key: ", key))),
        }
        Ok(())
    })?;
    let (Some(host), Some(port)) = (host, port) else {
        return Err(c.error_str("MemSql needs host and port"));
    };
    Ok(PDatasourceSpec::PMemSqlSpec(Box::new(PMemSqlSpec {
        database_name,
        host,
        port,
        use_ssl,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `parseOracleSpec`: `Oracle { host; port; serviceName?; };`.
fn parse_oracle_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut host, mut port, mut service_name) = (None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "host" => host = Some(string(c)?),
            "port" => port = Some(c.consume_long()?),
            "serviceName" => service_name = Some(string(c)?),
            _ => return Err(c.error(jcat!("unknown Oracle key: ", key))),
        }
        Ok(())
    })?;
    let (Some(host), Some(port)) = (host, port) else {
        return Err(c.error_str("Oracle needs host and port"));
    };
    Ok(PDatasourceSpec::POracleSpec(Box::new(POracleSpec {
        host,
        port,
        service_name,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `parseTrinoSpec`: `Trino { host; port; catalog?; schema?; clientTags?; sslSpecification: {...}?; };`.
fn parse_trino_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut host, mut port, mut catalog, mut schema, mut client_tags, mut ssl) = (None, None, None, None, None, None);
    spec_body(c, Some(keyword_tok), |c, k, key| {
        match k {
            "host" => host = Some(string(c)?),
            "port" => port = Some(c.consume_long()?),
            "catalog" => catalog = Some(string(c)?),
            "schema" => schema = Some(string(c)?),
            "clientTags" => client_tags = Some(string(c)?),
            "sslSpecification" => {
                c.expect(T::BRACE_OPEN)?;
                let (mut flag, mut path, mut pass) = (None, None, None);
                let mut seen: HashSet<Vec<u16>> = HashSet::new();
                while !c.at_end() && c.peek() != T::BRACE_CLOSE {
                    let sk = c.parse_identifier()?;
                    once_at(&mut seen, &sk, c.tokens(), keyword_tok)?;
                    c.expect(T::COLON)?;
                    match key_name(&sk).as_str() {
                        "ssl" => flag = Some(boolean_value(c)?),
                        "trustStorePathVaultReference" => path = Some(string(c)?),
                        "trustStorePasswordVaultReference" => pass = Some(string(c)?),
                        _ => return Err(c.error(jcat!("unknown sslSpecification key: ", sk))),
                    }
                    c.expect(T::SEMI_COLON)?;
                }
                c.expect(T::BRACE_CLOSE)?;
                let Some(flag) = flag else {
                    return Err(c.error_str("sslSpecification needs ssl"));
                };
                ssl = Some(Box::new(PTrinoSsl {
                    ssl: flag,
                    trust_store_path_vault_reference: path,
                    trust_store_password_vault_reference: pass,
                }));
            }
            _ => return Err(c.error(jcat!("unknown Trino key: ", key))),
        }
        Ok(())
    })?;
    let (Some(host), Some(port)) = (host, port) else {
        return Err(c.error_str("Trino needs host and port"));
    };
    Ok(PDatasourceSpec::PTrinoSpec(Box::new(PTrinoSpec {
        catalog,
        client_tags,
        host,
        port,
        schema,
        ssl_specification: ssl,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `Spanner { projectId; instanceId; databaseId; proxyHost?; proxyPort?; };`.
fn parse_spanner_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut project_id, mut instance_id, mut database_id, mut proxy_host, mut proxy_port) =
        (None, None, None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "projectId" => project_id = Some(string(c)?),
            "instanceId" => instance_id = Some(string(c)?),
            "databaseId" => database_id = Some(string(c)?),
            "proxyHost" => proxy_host = Some(string(c)?),
            "proxyPort" => proxy_port = Some(c.consume_long()?),
            _ => return Err(c.error(jcat!("unknown Spanner key: ", key))),
        }
        Ok(())
    })?;
    let (Some(project_id), Some(instance_id), Some(database_id)) = (project_id, instance_id, database_id) else {
        return Err(c.error_str("Spanner needs projectId, instanceId and databaseId"));
    };
    Ok(PDatasourceSpec::PSpannerSpec(Box::new(PSpannerSpec {
        database_id,
        instance_id,
        project_id,
        proxy_host,
        proxy_port,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `Databricks { hostname; port; protocol; httpPath; };`, the port a string.
fn parse_databricks_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut hostname, mut port, mut protocol, mut http_path) = (None, None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "hostname" => hostname = Some(string(c)?),
            "port" => port = Some(string(c)?),
            "protocol" => protocol = Some(string(c)?),
            "httpPath" => http_path = Some(string(c)?),
            _ => return Err(c.error(jcat!("unknown Databricks key: ", key))),
        }
        Ok(())
    })?;
    let (Some(hostname), Some(port), Some(protocol), Some(http_path)) = (hostname, port, protocol, http_path) else {
        return Err(c.error_str("Databricks needs hostname, port, protocol and httpPath"));
    };
    Ok(PDatasourceSpec::PDatabricksSpec(Box::new(PDatabricksSpec {
        hostname,
        http_path,
        port,
        protocol,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `BigQuery { projectId; defaultDataset; proxyHost?; proxyPort?; };`.
fn parse_big_query_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut project_id, mut default_dataset, mut proxy_host, mut proxy_port) = (None, None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "projectId" => project_id = Some(string(c)?),
            "defaultDataset" => default_dataset = Some(string(c)?),
            "proxyHost" => proxy_host = Some(string(c)?),
            "proxyPort" => proxy_port = Some(string_or_int(c)?),
            _ => return Err(c.error(jcat!("unknown BigQuery key: ", key))),
        }
        Ok(())
    })?;
    let (Some(project_id), Some(default_dataset)) = (project_id, default_dataset) else {
        return Err(c.error_str("BigQuery needs projectId and defaultDataset"));
    };
    Ok(PDatasourceSpec::PBigQuerySpec(Box::new(PBigQuerySpec {
        default_dataset,
        project_id,
        proxy_host,
        proxy_port,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

/// `EmbeddedH2 { name; directory; autoServerMode; };`.
fn parse_embedded_h2_spec<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PDatasourceSpec> {
    let (mut name, mut directory, mut auto_server) = (None, None, None);
    spec_body(c, None, |c, k, key| {
        match k {
            "name" => name = Some(string(c)?),
            "directory" => directory = Some(string(c)?),
            "autoServerMode" => auto_server = Some(boolean_value(c)?),
            _ => return Err(c.error(jcat!("unknown EmbeddedH2 key: ", key))),
        }
        Ok(())
    })?;
    let (Some(database_name), Some(directory), Some(auto_server_mode)) = (name, directory, auto_server) else {
        return Err(c.error_str("EmbeddedH2 needs name, directory and autoServerMode"));
    };
    Ok(PDatasourceSpec::PH2EmbeddedSpec(Box::new(PH2EmbeddedSpec {
        database_name,
        directory,
        auto_server_mode,
        source_information: c.span(keyword_tok, c.pos() - 1),
    })))
}

// ================================================================ auth strategies

/// An auth strategy's optional empty body `{}` and its `;` (DefaultH2, Test, GCPApplicationDefaultCredentials).
fn empty_body<'s>(c: &mut impl Cursor<'s>) -> R<()> {
    if c.peek() == T::BRACE_OPEN {
        c.advance()?;
        c.expect(T::BRACE_CLOSE)?;
    }
    c.expect(T::SEMI_COLON)
}

/// `parseAuthStrategy(c, keywordTok)`: the kind and its body; the span the `auth` keyword through the body's `;`.
fn parse_auth_strategy<'s>(c: &mut impl Cursor<'s>, keyword_tok: usize) -> R<PAuthStrategy> {
    let kind = c.parse_identifier()?;
    Ok(match key_name(&kind).as_str() {
        "DefaultH2" => {
            empty_body(c)?;
            PAuthStrategy::PH2Default(Box::new(PH2Default { source_information: c.span(keyword_tok, c.pos() - 1) }))
        }
        "Test" => {
            empty_body(c)?;
            PAuthStrategy::PTestAuth(Box::new(PTestAuth { source_information: c.span(keyword_tok, c.pos() - 1) }))
        }
        "OAuth" => {
            let (mut oauth_key, mut scope_name) = (None, None);
            spec_body(c, None, |c, _, key| {
                // the value is read before the key is judged
                let v = quoted_string(c)?;
                if eq(key, "oauthKey") {
                    oauth_key = Some(v);
                } else if eq(key, "scopeName") {
                    scope_name = Some(v);
                } else {
                    return Err(c.error(jcat!("unknown OAuth key: ", key)));
                }
                Ok(())
            })?;
            let (Some(oauth_key), Some(scope_name)) = (oauth_key, scope_name) else {
                return Err(c.error_str("OAuth needs oauthKey and scopeName"));
            };
            PAuthStrategy::POAuth(Box::new(POAuth {
                oauth_key,
                scope_name,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "DelegatedKerberos" => {
            let mut principal = None;
            if c.peek() == T::BRACE_OPEN {
                c.advance()?;
                let mut seen: HashSet<Vec<u16>> = HashSet::new();
                while !c.at_end() && c.peek() != T::BRACE_CLOSE {
                    let key = c.parse_identifier()?;
                    once(&mut seen, &key, c)?;
                    c.expect(T::COLON)?;
                    if !eq(&key, "serverPrincipal") {
                        return Err(c.error(jcat!("unknown DelegatedKerberos key: ", key)));
                    }
                    principal = Some(quoted_string(c)?);
                    c.expect(T::SEMI_COLON)?;
                }
                c.expect(T::BRACE_CLOSE)?;
            }
            c.expect(T::SEMI_COLON)?;
            PAuthStrategy::PDelegatedKerberos(Box::new(PDelegatedKerberos {
                server_principal: principal,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "UserNamePassword" => {
            let (mut base, mut user, mut pass) = (None, None, None);
            spec_body(c, None, |c, _, key| {
                let v = quoted_string(c)?;
                if eq(key, "baseVaultReference") {
                    base = Some(v);
                } else if eq(key, "userNameVaultReference") {
                    user = Some(v);
                } else if eq(key, "passwordVaultReference") {
                    pass = Some(v);
                } else {
                    return Err(c.error(jcat!("unknown UserNamePassword key: ", key)));
                }
                Ok(())
            })?;
            let (Some(user), Some(pass)) = (user, pass) else {
                return Err(c.error_str("UserNamePassword needs userNameVaultReference and passwordVaultReference"));
            };
            PAuthStrategy::PUserNamePassword(Box::new(PUserNamePassword {
                base_vault_reference: base,
                user_name_vault_reference: user,
                password_vault_reference: pass,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "SnowflakePublic" => {
            let (mut public_user_name, mut private_key, mut pass_phrase) = (None, None, None);
            spec_body(c, Some(keyword_tok), |c, k, key| {
                match k {
                    "publicUserName" => public_user_name = Some(string(c)?),
                    "privateKeyVaultReference" => private_key = Some(string(c)?),
                    "passPhraseVaultReference" => pass_phrase = Some(string(c)?),
                    _ => return Err(c.error(jcat!("unknown SnowflakePublic key: ", key))),
                }
                Ok(())
            })?;
            let (Some(public_user_name), Some(private_key), Some(pass_phrase)) =
                (public_user_name, private_key, pass_phrase)
            else {
                return Err(throw_at(
                    c.tokens(),
                    keyword_tok,
                    JStr::from(
                        "SnowflakePublic needs publicUserName, privateKeyVaultReference and passPhraseVaultReference",
                    ),
                ));
            };
            PAuthStrategy::PSnowflakePublic(Box::new(PSnowflakePublic {
                pass_phrase_vault_reference: pass_phrase,
                private_key_vault_reference: private_key,
                public_user_name,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "GCPApplicationDefaultCredentials" => {
            empty_body(c)?;
            PAuthStrategy::PGCPApplicationDefaultCredentials(Box::new(PGCPApplicationDefaultCredentials {
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "ApiToken" => {
            let mut api_token = None;
            spec_body(c, None, |c, _, key| {
                if !eq(key, "apiToken") {
                    return Err(c.error(jcat!("unknown ApiToken key: ", key)));
                }
                api_token = Some(string(c)?);
                Ok(())
            })?;
            let Some(api_token) = api_token else {
                return Err(c.error_str("ApiToken needs apiToken"));
            };
            PAuthStrategy::PApiToken(Box::new(PApiToken {
                api_token,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "MiddleTierUserNamePassword" => {
            let mut vault_reference = None;
            spec_body(c, None, |c, _, key| {
                if !eq(key, "vaultReference") {
                    return Err(c.error(jcat!("unknown MiddleTierUserNamePassword key: ", key)));
                }
                vault_reference = Some(string(c)?);
                Ok(())
            })?;
            let Some(vault_reference) = vault_reference else {
                return Err(c.error_str("MiddleTierUserNamePassword needs vaultReference"));
            };
            PAuthStrategy::PMiddleTierUserNamePassword(Box::new(PMiddleTierUserNamePassword {
                vault_reference,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "TrinoDelegatedKerberos" => {
            let (mut remote, mut canonical, mut server_principal) = (None, None, None);
            spec_body(c, Some(keyword_tok), |c, k, key| {
                match k {
                    "kerberosRemoteServiceName" => remote = Some(string(c)?),
                    "kerberosUseCanonicalHostname" => canonical = Some(boolean_value(c)?),
                    "serverPrincipal" => server_principal = Some(string(c)?),
                    _ => return Err(c.error(jcat!("unknown TrinoDelegatedKerberos key: ", key))),
                }
                Ok(())
            })?;
            PAuthStrategy::PTrinoKerberosAuth(Box::new(PTrinoKerberosAuth {
                kerberos_remote_service_name: remote,
                kerberos_use_canonical_hostname: canonical,
                server_principal,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        "GCPWorkloadIdentityFederation" => {
            let (mut email, mut scopes) = (None, None);
            spec_body(c, Some(keyword_tok), |c, k, key| {
                match k {
                    "serviceAccountEmail" => email = Some(string(c)?),
                    "additionalGcpScopes" => {
                        let mut list = Vec::new();
                        c.expect(T::BRACKET_OPEN)?;
                        while c.peek() != T::BRACKET_CLOSE {
                            list.push(string(c)?);
                            if !c.match_tok(T::COMMA)? {
                                break;
                            }
                        }
                        c.expect(T::BRACKET_CLOSE)?;
                        scopes = Some(list);
                    }
                    _ => return Err(c.error(jcat!("unknown GCPWorkloadIdentityFederation key: ", key))),
                }
                Ok(())
            })?;
            let Some(service_account_email) = email else {
                return Err(c.error_str("GCPWorkloadIdentityFederation needs serviceAccountEmail"));
            };
            PAuthStrategy::PGcpWifAuth(Box::new(PGcpWifAuth {
                additional_gcp_scopes: scopes,
                service_account_email,
                source_information: c.span(keyword_tok, c.pos() - 1),
            }))
        }
        _ => return Err(c.error(jcat!("unsupported auth strategy: ", kind, " (corpus-censused shapes only)"))),
    })
}

// ================================================================ authentication islands

/// `parseUserPasswordAuthIsland`: a MongoDB connection's authentication island, any registered kind.
fn parse_user_password_auth_island<'s>(c: &mut impl Cursor<'s>, key_tok: usize) -> R<PAuthSpecValue> {
    if c.peek() != T::ISLAND_OPEN {
        return Err(c.error_str("MongoDBConnection authentication must be a # <Kind> {...}# island"));
    }
    parse_es_auth_island(c, key_tok)
}

/// `authSpan`: the auth island's content span -- its first content token through the island's close `+3` (the
/// re-parse overshoot); an empty island anchors at its own `}#`.
fn auth_span<'s>(c: &impl Cursor<'s>, key_tok: usize, ip: &Relexed, ic: &IslandCursor<'_, '_>) -> Span {
    let a = c.span_of(key_tok, ip.end_tok);
    let (start_line, start_column) = if ic.tokens.count() == 0 {
        let ts = c.tokens();
        (ts.start_line(ip.end_tok) as i32, ts.start_column(ip.end_tok) as i32)
    } else {
        let first = ic.span_of(0, 0);
        (first.start_line, first.start_column)
    };
    boxed(SourceInfo {
        source_id: a.source_id,
        start_line,
        start_column,
        end_line: a.end_line,
        end_column: a.end_column + 3,
    })
}

/// `parseUserPasswordBody`: `# UserPassword { username: '...'; password: <secret>; }#`.
fn parse_user_password_body<'s>(c: &mut impl Cursor<'s>, key_tok: usize) -> R<PAuthSpecValue> {
    let ip = re_lex_island(c)?;
    let its = tokenize(&ip.text);
    let mut ic = ip.cursor(&its, c.dialect());
    let (mut username, mut secret) = (None, None);
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !ic.at_end() {
        let ik = ic.parse_identifier()?;
        once(&mut seen, &ik, &ic)?;
        ic.expect(T::COLON)?;
        if eq(&ik, "username") {
            username = Some(string(&mut ic)?);
            ic.expect(T::SEMI_COLON)?;
        } else if eq(&ik, "password") {
            secret = Some(parse_vault_secret(&mut ic)?);
        } else {
            return Err(ic.error(jcat!("unknown UserPassword key: ", ik)));
        }
    }
    let (Some(username), Some(password)) = (username, secret) else {
        return Err(c.error_str("UserPassword needs username and password"));
    };
    Ok(PAuthSpecValue::PMongoAuth(Box::new(PMongoAuth {
        username,
        password,
        source_information: auth_span(c, key_tok, &ip, &ic),
    })))
}

/// `parseElasticsearchBody`: `{ store: qn; clusterDetails: # URL { <uri> }#; authentication: # Kind {...}#; }`, each
/// field `;`-terminated and required, a duplicate refused at the first key; the value's span the first key through
/// the close, its end overshooting by 4.
fn parse_elasticsearch_body<'s>(c: &mut impl Cursor<'s>) -> R<PElasticsearchConnection> {
    c.expect(T::BRACE_OPEN)?;
    let first_key_tok = c.pos();
    let mut element = None;
    let mut element_span = None;
    let mut url = None;
    let mut auth = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_tok = c.pos();
        let key = c.parse_identifier()?;
        once_at(&mut seen, &key, c.tokens(), first_key_tok)?;
        c.expect(T::COLON)?;
        match key_name(&key).as_str() {
            "store" => {
                let e_s = c.pos();
                element = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
                element_span = c.span(e_s, c.pos() - 1);
            }
            "clusterDetails" => {
                if c.peek() != T::ISLAND_OPEN {
                    return Err(c.error_str("clusterDetails must be a # URL {...}# island"));
                }
                let kind_tok = c.pos();
                let kind = island_kind(c);
                if !eq(&kind, "URL") {
                    return Err(throw_at(
                        c.tokens(),
                        kind_tok,
                        jcat!("Unsupported cluster details type: ", kind, ".  Supported: URL"),
                    ));
                }
                // the raw source slice: a `//` in a url is no comment
                let (bs, end) = scan_island(c)?;
                let raw = trim(&c.reconstruct_text(bs, end)).to_vec();
                c.expect(T::ISLAND_END)?;
                if !uri_parses(&raw) {
                    let ts = c.tokens();
                    return Err(JavaError::parse(
                        JStr::from("URL is not valid"),
                        ts.start_line(bs) as i32,
                        ts.start_column(bs) as i32,
                    ));
                }
                url = Some(JStr(raw));
            }
            "authentication" => auth = Some(parse_es_auth_island(c, key_tok)?),
            _ => return Err(c.error(jcat!("unknown Elasticsearch7ClusterConnection key: ", key))),
        }
        c.expect(T::SEMI_COLON)?;
    }
    let close_tok = c.pos();
    c.expect(T::BRACE_CLOSE)?;
    let Some(element) = element else {
        return Err(throw_at(c.tokens(), first_key_tok, JStr::from("Field 'store' is required")));
    };
    let Some(url) = url else {
        return Err(throw_at(c.tokens(), first_key_tok, JStr::from("Field 'clusterDetails' is required")));
    };
    let Some(auth) = auth else {
        return Err(throw_at(c.tokens(), first_key_tok, JStr::from("Field 'authentication' is required")));
    };
    let vs = c.span_of(first_key_tok, close_tok);
    Ok(PElasticsearchConnection {
        element,
        element_source_information: element_span,
        url,
        auth,
        source_information: boxed(SourceInfo { end_column: vs.end_column + 4, ..vs }),
    })
}

/// `parseVaultSecret`: one secret -- `AWSSecretsManagerSecret { secretId; versionId?; versionStage?; awsCredentials:
/// Default {} | Static {...}; }` or a single-field kind (`PropertiesFileSecret`, `SystemPropertiesSecret`,
/// `EnvironmentSecret`), each `;`-terminated.
fn parse_vault_secret(ic: &mut IslandCursor<'_, '_>) -> R<PVaultSecret> {
    let v_s = ic.pos;
    let sk = ic.parse_identifier()?;
    if eq(&sk, "AWSSecretsManagerSecret") {
        ic.expect(T::BRACE_OPEN)?;
        let (mut secret_id, mut version_id, mut version_stage, mut creds_kind) = (None, None, None, None);
        let (mut access_key_id, mut secret_access_key, mut creds_span) = (None, None, None);
        while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
            let k = ic.parse_identifier()?;
            ic.expect(T::COLON)?;
            match key_name(&k).as_str() {
                "secretId" => {
                    secret_id = Some(string(ic)?);
                    ic.expect(T::SEMI_COLON)?;
                }
                "versionId" => {
                    version_id = Some(string(ic)?);
                    ic.expect(T::SEMI_COLON)?;
                }
                "versionStage" => {
                    version_stage = Some(string(ic)?);
                    ic.expect(T::SEMI_COLON)?;
                }
                "awsCredentials" => {
                    let c_s = ic.pos;
                    let ck = ic.parse_identifier()?;
                    ic.expect(T::BRACE_OPEN)?;
                    if eq(&ck, "Default") {
                        // spanless on the wire
                        creds_kind = Some(JStr::from("awsDefault"));
                        ic.expect(T::BRACE_CLOSE)?;
                    } else if eq(&ck, "Static") {
                        creds_kind = Some(JStr::from("awsStatic"));
                        while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
                            let ak = ic.parse_identifier()?;
                            ic.expect(T::COLON)?;
                            let PVaultSecret::PMongoSecret(ms) = parse_vault_secret(ic)? else {
                                return Err(ic.error_str("nested AWS credentials need a single-field secret"));
                            };
                            if eq(&ak, "accessKeyId") {
                                access_key_id = Some(ms);
                            } else if eq(&ak, "secretAccessKey") {
                                secret_access_key = Some(ms);
                            } else {
                                return Err(ic.error(jcat!("unknown Static key: ", ak)));
                            }
                        }
                        ic.expect(T::BRACE_CLOSE)?;
                        creds_span = ic.span(c_s, ic.pos - 1);
                    } else {
                        return Err(ic.error(jcat!("unsupported awsCredentials kind: ", ck)));
                    }
                }
                _ => return Err(ic.error(jcat!("unknown AWSSecretsManagerSecret key: ", k))),
            }
        }
        ic.expect(T::BRACE_CLOSE)?;
        let (Some(secret_id), Some(creds_kind)) = (secret_id, creds_kind) else {
            return Err(ic.error_str("AWSSecretsManagerSecret needs secretId and awsCredentials"));
        };
        ic.expect(T::SEMI_COLON)?;
        return Ok(PVaultSecret::PAwsSecret(Box::new(PAwsSecret {
            secret_id,
            version_id,
            version_stage,
            creds_kind,
            access_key_id,
            secret_access_key,
            creds_span,
        })));
    }
    let (wire_kind, wire_field) = match key_name(&sk).as_str() {
        "PropertiesFileSecret" => ("properties", "propertyName"),
        "SystemPropertiesSecret" => ("systemproperties", "systemPropertyName"),
        "EnvironmentSecret" => ("environment", "envVariableName"),
        _ => return Err(ic.error(jcat!("unsupported secret kind: ", sk))),
    };
    ic.expect(T::BRACE_OPEN)?;
    let field_key = ic.parse_identifier()?;
    ic.expect(T::COLON)?;
    let v = string(ic)?;
    ic.expect(T::SEMI_COLON)?;
    ic.expect(T::BRACE_CLOSE)?;
    if !eq(&field_key, wire_field) {
        return Err(ic.error(jcat!("unknown ", sk, " key: ", field_key)));
    }
    let secret = PMongoSecret {
        kind: JStr::from(wire_kind),
        field_key: JStr::from(wire_field),
        value: v,
        source_information: ic.span(v_s, ic.pos - 1),
    };
    ic.expect(T::SEMI_COLON)?;
    Ok(PVaultSecret::PMongoSecret(Box::new(secret)))
}

/// `parseAwsCredentials`: `Default {}` (spanless), `Static { accessKeyId; secretAccessKey; }` or `STSAssumeRole {
/// roleArn; roleSessionName; awsCredentials; }`, recursive; a span the kind through the close.
fn parse_aws_credentials(ic: &mut IslandCursor<'_, '_>) -> R<PAwsCredentials> {
    let k_s = ic.pos;
    let ck = ic.parse_identifier()?;
    match key_name(&ck).as_str() {
        "Default" => {
            ic.expect(T::BRACE_OPEN)?;
            ic.expect(T::BRACE_CLOSE)?;
            Ok(PAwsCredentials::PAwsDefault(Box::new(PAwsDefault {})))
        }
        "Static" => {
            ic.expect(T::BRACE_OPEN)?;
            let (mut access_key_id, mut secret_access_key) = (None, None);
            while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
                let k = ic.parse_identifier()?;
                ic.expect(T::COLON)?;
                let PVaultSecret::PMongoSecret(ms) = parse_vault_secret(ic)? else {
                    return Err(ic.error_str("Static credentials need single-field secrets"));
                };
                if eq(&k, "accessKeyId") {
                    access_key_id = Some(ms);
                } else if eq(&k, "secretAccessKey") {
                    secret_access_key = Some(ms);
                } else {
                    return Err(ic.error(jcat!("unknown Static key: ", k)));
                }
            }
            ic.expect(T::BRACE_CLOSE)?;
            let (Some(access_key_id), Some(secret_access_key)) = (access_key_id, secret_access_key) else {
                return Err(ic.error_str("Static needs accessKeyId and secretAccessKey"));
            };
            Ok(PAwsCredentials::PAwsStatic(Box::new(PAwsStatic {
                access_key_id,
                secret_access_key,
                source_information: ic.span(k_s, ic.pos - 1),
            })))
        }
        "STSAssumeRole" => {
            ic.expect(T::BRACE_OPEN)?;
            let (mut role_arn, mut role_session_name, mut nested) = (None, None, None);
            while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
                let k = ic.parse_identifier()?;
                ic.expect(T::COLON)?;
                match key_name(&k).as_str() {
                    "roleArn" => {
                        role_arn = Some(string(ic)?);
                        ic.expect(T::SEMI_COLON)?;
                    }
                    "roleSessionName" => {
                        role_session_name = Some(string(ic)?);
                        ic.expect(T::SEMI_COLON)?;
                    }
                    "awsCredentials" => nested = Some(parse_aws_credentials(ic)?),
                    _ => return Err(ic.error(jcat!("unknown STSAssumeRole key: ", k))),
                }
            }
            ic.expect(T::BRACE_CLOSE)?;
            let (Some(role_arn), Some(role_session_name), Some(aws_credentials)) =
                (role_arn, role_session_name, nested)
            else {
                return Err(ic.error_str("STSAssumeRole needs roleArn, roleSessionName and awsCredentials"));
            };
            Ok(PAwsCredentials::PAwsStsRole(Box::new(PAwsStsRole {
                role_arn,
                role_session_name,
                aws_credentials,
                source_information: ic.span(k_s, ic.pos - 1),
            })))
        }
        _ => Err(ic.error(jcat!("unsupported awsCredentials kind: ", ck))),
    }
}

/// `parseEsAuthIsland`: the general authentication island -- any registered kind (UserPassword, Kerberos, ApiKey,
/// GCPWIFWithAWSIdP, PSK, EncryptedPrivateKey).
fn parse_es_auth_island<'s>(c: &mut impl Cursor<'s>, key_tok: usize) -> R<PAuthSpecValue> {
    if c.peek() != T::ISLAND_OPEN {
        return Err(c.error_str("authentication must be a # <Kind> {...}# island"));
    }
    let kind = island_kind(c);
    match key_name(&kind).as_str() {
        "UserPassword" => parse_user_password_body(c, key_tok),
        "Kerberos" => {
            let ip = re_lex_island(c)?;
            let its = tokenize(&ip.text);
            let ic = ip.cursor(&its, c.dialect());
            if !ic.at_end() {
                return Err(c.error_str("Kerberos auth takes no fields"));
            }
            Ok(PAuthSpecValue::PKerberosAuth(Box::new(PKerberosAuth {
                source_information: auth_span(c, key_tok, &ip, &ic),
            })))
        }
        "ApiKey" => {
            let ip = re_lex_island(c)?;
            let its = tokenize(&ip.text);
            let mut ic = ip.cursor(&its, c.dialect());
            let (mut location, mut key_name_v, mut value) = (None, None, None);
            let mut seen: HashSet<Vec<u16>> = HashSet::new();
            while !ic.at_end() {
                let ik = ic.parse_identifier()?;
                once(&mut seen, &ik, &ic)?;
                ic.expect(T::COLON)?;
                match key_name(&ik).as_str() {
                    "location" => {
                        // `Location.valueOf(upper)`: HEADER or COOKIE
                        let l = to_upper_root(&string_value(&mut ic)?);
                        if !eq(&l, "HEADER") && !eq(&l, "COOKIE") {
                            return Err(ic.error(jcat!("unknown ApiKey location: ", l)));
                        }
                        location = Some(JStr(l));
                        ic.expect(T::SEMI_COLON)?;
                    }
                    "keyName" => {
                        key_name_v = Some(string(&mut ic)?);
                        ic.expect(T::SEMI_COLON)?;
                    }
                    "value" => value = Some(parse_vault_secret(&mut ic)?),
                    _ => return Err(ic.error(jcat!("unknown ApiKey key: ", ik))),
                }
            }
            let (Some(location), Some(key_name_v), Some(value)) = (location, key_name_v, value) else {
                return Err(c.error_str("ApiKey needs location, keyName and value"));
            };
            Ok(PAuthSpecValue::PApiKeyAuth(Box::new(PApiKeyAuth {
                key_name: key_name_v,
                location,
                value,
                source_information: auth_span(c, key_tok, &ip, &ic),
            })))
        }
        "GCPWIFWithAWSIdP" => parse_gcp_wif_island(c, key_tok),
        "PSK" => {
            // `PSK COLON STRING SEMI_COLON` with no EOF: the rest of the island is ignored
            let ip = re_lex_island(c)?;
            let its = tokenize(&ip.text);
            let mut ic = ip.cursor(&its, c.dialect());
            let ik = ic.parse_identifier()?;
            if !eq(&ik, "psk") {
                return Err(ic.error(jcat!("unknown PSK key: ", ik)));
            }
            ic.expect(T::COLON)?;
            let psk = string(&mut ic)?;
            ic.expect(T::SEMI_COLON)?;
            while !ic.at_end() {
                ic.advance()?;
            }
            Ok(PAuthSpecValue::PPskAuth(Box::new(PPskAuth { psk })))
        }
        "EncryptedPrivateKey" => {
            let ip = re_lex_island(c)?;
            let its = tokenize(&ip.text);
            let mut ic = ip.cursor(&its, c.dialect());
            let (mut user_name, mut private_key, mut passphrase) = (None, None, None);
            let mut seen: HashSet<Vec<u16>> = HashSet::new();
            while !ic.at_end() {
                let ik = ic.parse_identifier()?;
                once(&mut seen, &ik, &ic)?;
                ic.expect(T::COLON)?;
                match key_name(&ik).as_str() {
                    "userName" => {
                        user_name = Some(string(&mut ic)?);
                        ic.expect(T::SEMI_COLON)?;
                    }
                    "privateKey" => private_key = Some(parse_vault_secret(&mut ic)?),
                    "passphrase" => passphrase = Some(parse_vault_secret(&mut ic)?),
                    _ => return Err(ic.error(jcat!("unknown EncryptedPrivateKey key: ", ik))),
                }
            }
            let (Some(user_name), Some(private_key), Some(passphrase)) = (user_name, private_key, passphrase) else {
                return Err(c.error_str("EncryptedPrivateKey needs userName, privateKey and passphrase"));
            };
            Ok(PAuthSpecValue::PEpkAuth(Box::new(PEpkAuth {
                user_name,
                private_key,
                passphrase,
                source_information: auth_span(c, key_tok, &ip, &ic),
            })))
        }
        _ => Err(c.error(jcat!("unsupported auth island kind: ", kind))),
    }
}

/// The `GCPWIFWithAWSIdP` island: `serviceAccountEmail; idP: AWSIdP { accountId; region; role; awsCredentials; }
/// workload: GCPWorkload { projectNumber; providerId; poolId; }`, all required.
fn parse_gcp_wif_island<'s>(c: &mut impl Cursor<'s>, key_tok: usize) -> R<PAuthSpecValue> {
    let ip = re_lex_island(c)?;
    let its = tokenize(&ip.text);
    let mut ic = ip.cursor(&its, c.dialect());
    let (mut email, mut account_id, mut region, mut role, mut creds) = (None, None, None, None, None);
    let (mut project_number, mut provider_id, mut pool_id) = (None, None, None);
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !ic.at_end() {
        let ik = ic.parse_identifier()?;
        once(&mut seen, &ik, &ic)?;
        ic.expect(T::COLON)?;
        match key_name(&ik).as_str() {
            "serviceAccountEmail" => {
                email = Some(string(&mut ic)?);
                ic.expect(T::SEMI_COLON)?;
            }
            "idP" => {
                let ipk = ic.parse_identifier()?;
                if !eq(&ipk, "AWSIdP") {
                    return Err(ic.error(jcat!("unknown idP kind: ", ipk)));
                }
                ic.expect(T::BRACE_OPEN)?;
                let mut seen_idp: HashSet<Vec<u16>> = HashSet::new();
                while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
                    let k2 = ic.parse_identifier()?;
                    once(&mut seen_idp, &k2, &ic)?;
                    ic.expect(T::COLON)?;
                    match key_name(&k2).as_str() {
                        "accountId" => {
                            account_id = Some(string(&mut ic)?);
                            ic.expect(T::SEMI_COLON)?;
                        }
                        "region" => {
                            region = Some(string(&mut ic)?);
                            ic.expect(T::SEMI_COLON)?;
                        }
                        "role" => {
                            role = Some(string(&mut ic)?);
                            ic.expect(T::SEMI_COLON)?;
                        }
                        "awsCredentials" => creds = Some(parse_aws_credentials(&mut ic)?),
                        _ => return Err(ic.error(jcat!("unknown AWSIdP key: ", k2))),
                    }
                }
                ic.expect(T::BRACE_CLOSE)?;
            }
            "workload" => {
                let wk = ic.parse_identifier()?;
                if !eq(&wk, "GCPWorkload") {
                    return Err(ic.error(jcat!("unknown workload kind: ", wk)));
                }
                ic.expect(T::BRACE_OPEN)?;
                let mut seen_w: HashSet<Vec<u16>> = HashSet::new();
                while !ic.at_end() && ic.peek() != T::BRACE_CLOSE {
                    let k3 = ic.parse_identifier()?;
                    once(&mut seen_w, &k3, &ic)?;
                    ic.expect(T::COLON)?;
                    let v = string(&mut ic)?;
                    ic.expect(T::SEMI_COLON)?;
                    match key_name(&k3).as_str() {
                        "projectNumber" => project_number = Some(v),
                        "providerId" => provider_id = Some(v),
                        "poolId" => pool_id = Some(v),
                        _ => return Err(ic.error(jcat!("unknown GCPWorkload key: ", k3))),
                    }
                }
                ic.expect(T::BRACE_CLOSE)?;
            }
            _ => return Err(ic.error(jcat!("unknown GCPWIFWithAWSIdP key: ", ik))),
        }
    }
    let (
        Some(service_account_email),
        Some(account_id),
        Some(region),
        Some(role),
        Some(aws_credentials),
        Some(project_number),
        Some(provider_id),
        Some(pool_id),
    ) = (email, account_id, region, role, creds, project_number, provider_id, pool_id)
    else {
        return Err(c.error_str("GCPWIFWithAWSIdP needs serviceAccountEmail, idP and workload"));
    };
    Ok(PAuthSpecValue::PGcpWifIslandAuth(Box::new(PGcpWifIslandAuth {
        service_account_email,
        account_id,
        region,
        role,
        aws_credentials,
        project_number,
        provider_id,
        pool_id,
        source_information: auth_span(c, key_tok, &ip, &ic),
    })))
}
