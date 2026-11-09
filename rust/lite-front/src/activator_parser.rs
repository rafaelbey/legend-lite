// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The function-activator sections (core/'s `parser.section.FunctionActivatorSectionGrammar`, docs/rust-port/PHASE2.md
//! §4 step 4d): `###Snowflake` (SnowflakeApp, SnowflakeM2MUdf), `###MemSql` (MemSqlFunction), `###BigQuery`
//! (BigQueryFunction), `###HostedService` (HostedService, and the Service grammar's ExecutionEnvironment), `###FunctionJar`,
//! each section's deployment configurations, and `###Deephaven`'s DeephavenApp -- one body shape: typed string and
//! boolean keys, a `function:` pointer keeping its signature's text, the ownership, an activation connection. Java's
//! answers reproduced, each refusal at Java's position.

use crate::cursor::{Cursor, R, is_identifier_token, once, refused_at, throw_at, unquote_path_iae};
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::TokenType;
use crate::records::{Element, PFunctionActivator};
use crate::sections::{boolean_value, linked_put, split_last, string_value};
use crate::service_parser::parse_execution_environment;
use crate::ustr::{eq, starts_with};
use std::collections::HashSet;

use TokenType as T;

/// `STRING_KEYS`.
const STRING_KEYS: &[&str] = &[
    "applicationName",
    "udfName",
    "functionName",
    "description",
    "documentation",
    "pattern",
    "deploymentSchema",
    "deploymentStage",
    "usageRole",
];

/// `BOOLEAN_KEYS`.
const BOOLEAN_KEYS: &[&str] = &["autoActivateUpdates", "generateLineage", "storeModel"];

/// `DEPLOYMENT_CONFIG_KINDS`.
const DEPLOYMENT_CONFIG_KINDS: &[&str] = &[
    "SnowflakeAppDeploymentConfiguration",
    "BigQueryFunctionDeploymentConfiguration",
    "MemSqlFunctionDeploymentConfiguration",
    "HostedServiceDeploymentConfiguration",
];

fn listed(list: &[&str], v: &[u16]) -> bool {
    list.iter().any(|s| eq(v, s))
}

/// `FunctionActivatorSectionGrammar.parseElement`: one `Kind <<dec>> qn { key: value; ... }` activator of `section`
/// (its admitted `kinds`), an `ExecutionEnvironment` (the Service grammar's), or a deployment configuration.
pub fn parse_element<'s>(c: &mut impl Cursor<'s>, section: &str, kinds: &[&str]) -> R<Element> {
    let decl_start = c.pos();
    if !is_identifier_token(c.peek()) {
        return Err(c.error(jcat!("unsupported ###", section, " element: ", c.safe_text())));
    }
    let kind = c.safe_text();
    if !listed(kinds, &kind) {
        return Err(c.error(jcat!("unsupported ###", section, " element: ", kind)));
    }
    if eq(&kind, "ExecutionEnvironment") {
        return Ok(Element::PExecutionEnvironment(Box::new(parse_execution_environment(
            c,
        )?)));
    }
    if listed(DEPLOYMENT_CONFIG_KINDS, &kind) {
        return Ok(Element::PFunctionActivator(Box::new(parse_deployment_config(
            c, &kind,
        )?)));
    }
    c.advance()?;
    let dec = c.parse_decorations()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut scalars: Vec<(JStr, JStr)> = Vec::new();
    let mut booleans: Vec<(JStr, bool)> = Vec::new();
    let mut function: Option<(Vec<u16>, crate::records::SourceInfo)> = None;
    let mut owner_id = None;
    let mut user_list_users: Option<Vec<JStr>> = None;
    let mut act_conn = None;
    let mut act_conn_span = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    let hosted = section == "HostedService";
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_tok = c.pos();
        let key = c.parse_identifier()?;
        if !(eq(&kind, "FunctionJar") && eq(&key, "activationConfiguration")) {
            once(&mut seen, &key, c)?;
        }
        c.expect(T::COLON)?;
        if listed(STRING_KEYS, &key) {
            let v = string_value(c)?;
            linked_put(&mut scalars, JStr(key), JStr(v));
        } else if listed(BOOLEAN_KEYS, &key) {
            let v = boolean_value(c)?;
            linked_put(&mut booleans, JStr(key), v);
        } else if eq(&key, "function") {
            let fd = c.parse_function_descriptor()?;
            if section == "Deephaven" {
                for i in fd.name_end..fd.end {
                    if eq(c.tokens().text(i), "..") {
                        return Err(throw_at(c.tokens(), i, JStr::from("Unexpected token '..'")));
                    }
                }
            }
            let mut path = unquote_path_iae(&c.reconstruct_text(fd.start, fd.name_end))?;
            path.extend(c.compact_text(fd.name_end, fd.end - 1));
            function = Some((path, c.span_of(fd.start, fd.end - 1)));
        } else if eq(&key, "ownership") {
            let o_kind = c.parse_identifier()?;
            c.expect(T::BRACE_OPEN)?;
            let ik = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if eq(&o_kind, "Deployment") {
                if !eq(&ik, "identifier") {
                    return Err(c.error(jcat!("unknown Deployment key: ", ik)));
                }
                owner_id = Some(JStr(string_value(c)?));
            } else if eq(&o_kind, "UserList") && hosted {
                if !eq(&ik, "users") {
                    return Err(c.error(jcat!("unknown UserList key: ", ik)));
                }
                let mut users = Vec::new();
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE {
                    users.push(JStr(string_value(c)?));
                    if !c.match_tok(T::COMMA)? {
                        break;
                    }
                }
                user_list_users = Some(users);
                c.expect(T::BRACKET_CLOSE)?;
            } else {
                return Err(c.error(jcat!("unsupported ownership kind: ", o_kind)));
            }
            c.match_tok(T::SEMI_COLON)?;
            c.expect(T::BRACE_CLOSE)?;
        } else if eq(&key, "permissionScheme") {
            let scheme = c.parse_identifier()?;
            if !eq(&scheme, "DEFAULT") && !eq(&scheme, "SEQUESTERED") {
                return Err(c.error(jcat!("Unknown permission scheme '", scheme, "'")));
            }
            linked_put(&mut scalars, JStr(key), JStr(scheme));
        } else if eq(&key, "activationConfiguration") {
            // the engine's DeephavenApp has no such key (the user's ruling 12 of 2026-11-09,
            // docs/rust-port/PHASE2.md section 6; PARITY row 71)
            if eq(&kind, "DeephavenApp") && c.dialect().answers_engine_json() {
                return Err(refused_at(c.tokens(), key_tok, jcat!("Unexpected token '", key, "'")));
            }
            let a_s = c.pos();
            act_conn = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
            if starts_with(&kind, "Snowflake") {
                act_conn_span = c.span(a_s, c.pos() - 1);
            }
        } else if hosted && eq(&key, "contentType") {
            string_value(c)?;
        } else if hosted && eq(&key, "binding") {
            c.parse_qualified_name()?;
            c.expect(T::SEMI_COLON)?;
        } else if hosted && (eq(&key, "postValidations") || eq(&key, "testSuites")) {
            c.skip_balanced_block()?;
            c.match_tok(T::SEMI_COLON)?;
            continue;
        } else {
            return Err(c.error(jcat!("unknown key '", key, "' inside ", kind, " '", qn, "'")));
        }
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some((function_path, function_span)) = function else {
        return Err(throw_at(
            c.tokens(),
            decl_start,
            jcat!(kind, " '", qn, "' needs a function"),
        ));
    };
    if owner_id.is_none() && user_list_users.is_none() && !eq(&kind, "BigQueryFunction") {
        return Err(throw_at(
            c.tokens(),
            decl_start,
            JStr::from("Field 'ownership' is required"),
        ));
    }
    let has = |k: &str| scalars.iter().any(|(s, _)| *s == k);
    let required = |f: &str| throw_at(c.tokens(), decl_start, jcat!("Field '", f, "' is required"));
    if eq(&kind, "DeephavenApp") && !has("applicationName") {
        return Err(required("applicationName"));
    }
    if eq(&kind, "MemSqlFunction") && !has("functionName") {
        return Err(required("functionName"));
    }
    if eq(&kind, "SnowflakeM2MUdf") {
        if !has("deploymentSchema") {
            return Err(required("deploymentSchema"));
        }
        if !has("deploymentStage") {
            return Err(required("deploymentStage"));
        }
    }
    Ok(Element::PFunctionActivator(Box::new(activator(PFunctionActivator {
        pkg: JStr(pkg),
        name: JStr(name),
        kind: JStr(kind),
        stereotypes: dec.stereotypes,
        tagged_values: dec.tagged_values,
        scalars,
        booleans,
        function_path: JStr(function_path),
        function_span: Some(Box::new(function_span)),
        owner_id,
        user_list_users,
        activation_connection: act_conn,
        activation_connection_span: act_conn_span,
        source_information: c.span(decl_start, c.pos() - 1),
    })?)))
}

/// `PFunctionActivator`'s compact constructor: the ownership is a Deployment or a UserList, never both.
fn activator(a: PFunctionActivator) -> R<PFunctionActivator> {
    // both would take a second `ownership`, which `once` refuses first, and a deployment configuration writes neither
    assert!(
        a.owner_id.is_none() || a.user_list_users.is_none(),
        "ownership is Deployment OR UserList, not both"
    );
    Ok(a)
}

/// `parseDeploymentConfig`: `XDeploymentConfiguration qn { activationConnection: qn; }`, the connection required but
/// for HostedService's; the record a nameless activator, its function span and its own the element's.
fn parse_deployment_config<'s>(c: &mut impl Cursor<'s>, kind: &[u16]) -> R<PFunctionActivator> {
    let decl_start = c.pos();
    c.advance()?;
    let qn = unquote_path_iae(&c.parse_qualified_name()?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut conn = None;
    let mut conn_span = None;
    let hosted = eq(kind, "HostedServiceDeploymentConfiguration");
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !eq(&key, "activationConnection") {
            return Err(c.error(jcat!("unknown ", kind, " key: ", key)));
        }
        let a_s = c.pos();
        conn = Some(JStr(unquote_path_iae(&c.parse_qualified_name()?)?));
        conn_span = c.span(a_s, c.pos() - 1);
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    if conn.is_none() && !hosted {
        return Err(throw_at(
            c.tokens(),
            decl_start,
            JStr::from("Field 'activationConnection' is required"),
        ));
    }
    let span = c.span(decl_start, c.pos() - 1);
    activator(PFunctionActivator {
        pkg: JStr(pkg),
        name: JStr(name),
        kind: JStr(kind.to_vec()),
        stereotypes: Vec::new(),
        tagged_values: Vec::new(),
        scalars: Vec::new(),
        booleans: Vec::new(),
        function_path: JStr::default(),
        function_span: span.clone(),
        owner_id: None,
        user_list_users: None,
        activation_connection: conn,
        activation_connection_span: conn_span,
        source_information: span,
    })
}
