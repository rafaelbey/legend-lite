// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `###Persistence` (core/'s `parser.section.PersistenceSectionGrammar`, docs/rust-port/PHASE2.md §4 step 4d):
//! `Persistence` (its documentation, service, trigger, the persister tree, the service-output targets, the notifier,
//! the tests) and `PersistenceContext` (the persistence, a platform, service parameters, a sink connection). The deep
//! sub-languages ride one generic node tree (`PPersistenceNode`: a kind, its entries sorted by key as its compact
//! constructor sorts them), islands re-lexed with their offsets composed through nesting, then held to the engine
//! walker's cardinality tables and given the spans the engine's JSON carries. Java's answers reproduced, each refusal at
//! Java's position.

// The generated records hold their record-typed list elements boxed (`Vec<Box<T>>`), so the readers build that.
#![allow(clippy::vec_box)]

use crate::connection_parser::parse_island_value;
use crate::cursor::{
    Cursor, IslandCursor, R, is_identifier_token, refused_at, refused_at_eof, throw_at, unquote_path_iae,
};
use crate::doors::JavaError;
use crate::jcat;
use crate::jtypes::JStr;
use crate::lexer::{TokenStream, TokenType, tokenize};
use crate::records::{
    Element, PConnectionPointer, PConnectionValue, PCtxParam, PCtxParamValue, PCtxParamValueConnectionPtr,
    PCtxParamValueConnectionVal, PCtxParamValuePrimitive, PPersistence, PPersistenceAssert, PPersistenceContext,
    PPersistenceEntry, PPersistenceEntryNode, PPersistenceEntryNodeList, PPersistenceEntryPathList,
    PPersistenceEntryPathValue, PPersistenceEntryPointer, PPersistenceEntryScalar, PPersistenceEntryStrings,
    PPersistenceNode, PPersistenceNotifier, PPersistenceTest, PPersistenceTestBatch, PServiceOutputTarget, SourceInfo,
    ValueSpecification,
};
use crate::sections::{split_last, string_value};
use crate::spec_parser::parse_tokens;
use crate::ustr::{eq, index_of_units, starts_with, u};
use lite_javacompat::character::{is_lower_case, is_upper_case};
use std::collections::HashSet;

use TokenType as T;

/// `PersistenceSectionGrammar.parseOne` and `qualifiedNameOf`: a `Persistence` or a `PersistenceContext`.
pub fn parse_element<'s>(c: &mut impl Cursor<'s>) -> R<(Element, Vec<u16>)> {
    if !is_identifier_token(c.peek()) {
        return Err(c.error(jcat!("unsupported ###Persistence element: ", c.safe_text())));
    }
    let kind = c.safe_text();
    if eq(&kind, "Persistence") {
        let p = parse_persistence(c)?;
        let path = crate::protocol::qualified_name(&p.pkg.0, &p.name.0);
        return Ok((Element::PPersistence(Box::new(p)), path));
    }
    if eq(&kind, "PersistenceContext") {
        let p = parse_context(c)?;
        let path = crate::protocol::qualified_name(&p.pkg.0, &p.name.0);
        return Ok((Element::PPersistenceContext(Box::new(p)), path));
    }
    Err(c.error(jcat!("unsupported ###Persistence element: ", c.safe_text())))
}

/// `TokenStreamCursor.parseQualifiedNameAdmittingBooleans`: `parseQualifiedName` with a `true` or `false` segment
/// admitted, and no leading check (a bad first token is `fqnSegmentText`'s refusal).
fn qn_admitting_booleans<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    let mut sb = bool_or_fqn_segment_text(c)?;
    while c.peek() == T::PATH_SEPARATOR {
        c.advance()?;
        sb.extend(u("::"));
        sb.extend(bool_or_fqn_segment_text(c)?);
    }
    Ok(sb)
}

fn bool_or_fqn_segment_text<'s>(c: &mut impl Cursor<'s>) -> R<Vec<u16>> {
    if c.peek() == T::TRUE || c.peek() == T::FALSE {
        let seg = c.text().to_vec();
        c.advance()?;
        return Ok(seg);
    }
    c.fqn_segment_text()
}

/// An island open at the end of the input, refused there as the engine refuses it (PARITY row 55).
fn open_island(ts: &TokenStream<'_>) -> JavaError {
    refused_at_eof(ts, jcat!("expected ", T::ISLAND_END, " but found EOF ('<EOF>')"))
}

fn start_line(ts: &TokenStream<'_>, i: usize) -> R<i32> {
    if i < ts.count() {
        Ok(ts.start_line(i) as i32)
    } else {
        Err(open_island(ts))
    }
}

fn start_column(ts: &TokenStream<'_>, i: usize) -> R<i32> {
    if i < ts.count() {
        Ok(ts.start_column(i) as i32)
    } else {
        Err(open_island(ts))
    }
}

/// `SpecParser.parse(tokens.slice(at, at + 1), dialect)`: one token's expression; at the end of the input refused
/// there, where a path literal was to come (PARITY row 56).
fn one_token_spec<'s>(c: &impl Cursor<'s>, at: usize) -> R<ValueSpecification> {
    if at >= c.tokens().count() {
        return Err(refused_at_eof(
            c.tokens(),
            jcat!("expected ", T::PATH_LITERAL, " but found EOF ('<EOF>')"),
        ));
    }
    parse_tokens(&c.tokens().slice(at, at + 1), c.dialect())
}

/// An island's extent: past the `#{` (or `#...{`) at the cursor to its own `}#` (nested `#{`...`}#` counted), not
/// consumed -- the content's first token.
fn scan_island<'s>(c: &mut impl Cursor<'s>) -> R<usize> {
    c.advance()?;
    let emb_start = c.pos();
    let mut depth = 0i64;
    while !c.at_end() {
        let t = c.peek();
        if t == T::ISLAND_START {
            depth += 1;
        } else if t == T::ISLAND_END {
            if depth == 0 {
                break;
            }
            depth -= 1;
        }
        c.advance()?;
    }
    Ok(emb_start)
}

fn is_island(t: TokenType) -> bool {
    t == T::ISLAND_OPEN || t == T::ISLAND_START
}

fn entry_key(e: &PPersistenceEntry) -> &[u16] {
    match e {
        PPersistenceEntry::PPersistenceEntryScalar(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryNode(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryPointer(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryStrings(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryPathValue(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryNodeList(x) => &x.key.0,
        PPersistenceEntry::PPersistenceEntryPathList(x) => &x.key.0,
    }
}

/// `new PPersistenceNode(kind, headPath, entries, span)`: the compact constructor sorts the entries by key
/// (`String.compareTo`, a stable sort).
fn node(
    kind: JStr,
    head_path: Option<ValueSpecification>,
    mut entries: Vec<PPersistenceEntry>,
    span: Option<Box<SourceInfo>>,
) -> PPersistenceNode {
    entries.sort_by(|a, b| entry_key(a).cmp(entry_key(b)));
    PPersistenceNode {
        kind,
        head_path,
        entries,
        source_information: span,
    }
}

fn node_entry(key: &str, n: PPersistenceNode) -> PPersistenceEntry {
    PPersistenceEntry::PPersistenceEntryNode(Box::new(PPersistenceEntryNode {
        key: JStr::from(key),
        node: Box::new(n),
    }))
}

/// `NO_DEDUPLICATION`: a flat target's default deduplication, spanless (as the sentinel `KEYLESS_TARGET`, an empty
/// `{ }` target): the two nodes no author wrote, which validation passes by (Java tests them by identity; no parsed
/// node is spanless before `wireSpans`, so their missing span tells them apart here).
fn no_deduplication() -> PPersistenceNode {
    node(JStr::from("None"), None, Vec::new(), None)
}

fn keyless_target() -> PPersistenceNode {
    node(JStr::from("__empty__"), None, Vec::new(), None)
}

/// `parsePersistence`: `Persistence <<dec>> qn { doc: '..'; trigger: Manual; service: qn; persister: ...;
/// serviceOutputTargets: [...]; notifier: {...}; tests: [...] }`, each key once (at the declaration), the trio
/// required; the persister and the output targets validated, then given their wire spans.
fn parse_persistence<'s>(c: &mut impl Cursor<'s>) -> R<PPersistence> {
    let decl_start = c.pos();
    c.advance()?;
    let dec = c.parse_decorations()?;
    let qn = unquote_path_iae(&qn_admitting_booleans(c)?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut doc = None;
    let mut trigger_kind = None;
    let mut service = None;
    let mut service_span = None;
    let mut persister = None;
    let mut notifier = None;
    let mut output_targets: Option<Vec<PServiceOutputTarget>> = None;
    let mut tests = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_start = c.pos();
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(throw_at(
                c.tokens(),
                decl_start,
                jcat!("Field '", key, "' should be specified only once"),
            ));
        }
        match String::from_utf16_lossy(&key).as_str() {
            "doc" => {
                doc = Some(JStr(string_value(c)?));
                c.expect(T::SEMI_COLON)?;
            }
            "service" => {
                service = Some(JStr(unquote_path_iae(&qn_admitting_booleans(c)?)?));
                c.expect(T::SEMI_COLON)?;
                service_span = c.span(key_start, c.pos() - 1);
            }
            "trigger" => {
                let t_tok = c.pos();
                let tk = c.parse_identifier()?;
                if !eq(&tk, "Manual") && !eq(&tk, "Cron") {
                    return Err(throw_at(c.tokens(), t_tok, jcat!("Unexpected token '", tk, "'")));
                }
                trigger_kind = Some(JStr(tk));
                c.expect(T::SEMI_COLON)?;
            }
            "persister" => {
                persister = Some(parse_node(c, (0, 0))?);
                c.match_tok(T::SEMI_COLON)?;
            }
            "serviceOutputTargets" => {
                output_targets = Some(parse_output_targets(c)?);
                c.match_tok(T::SEMI_COLON)?;
            }
            "notifier" => {
                notifier = Some(Box::new(parse_notifier(c, key_start)?));
                c.match_tok(T::SEMI_COLON)?;
            }
            "tests" => tests = Some(parse_tests(c)?),
            _ => return Err(c.error(jcat!("unknown key '", key, "' inside Persistence '", qn, "'"))),
        }
    }
    c.expect(T::BRACE_CLOSE)?;
    for r in ["doc", "trigger", "service"] {
        if !seen.contains(&u(r)) {
            return Err(throw_at(c.tokens(), decl_start, jcat!("Field '", r, "' is required")));
        }
    }
    let persister = match persister {
        Some(p) => {
            validate_node("persister", &p)?;
            Some(Box::new(wire_spans(p, &u("persister"), false)))
        }
        None => None,
    };
    let output_targets = match output_targets {
        Some(spanned) => {
            let mut out = Vec::new();
            for t in spanned {
                validate_node("serviceOutput", &t.service_output)?;
                validate_node("target", &t.persistence_target)?;
                out.push(Box::new(PServiceOutputTarget {
                    service_output: Box::new(wire_spans(*t.service_output, &u("serviceOutput"), false)),
                    persistence_target: Box::new(wire_spans(*t.persistence_target, &u("persistenceTarget"), true)),
                    source_information: t.source_information,
                }));
            }
            Some(out)
        }
        None => None,
    };
    Ok(PPersistence {
        pkg: JStr(pkg),
        name: JStr(name),
        stereotypes: dec.stereotypes,
        tagged_values: dec.tagged_values,
        doc,
        trigger_kind: trigger_kind.expect("a seen trigger is read"),
        service,
        service_span,
        persister,
        notifier,
        service_output_targets: output_targets,
        tests,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// `REQUIRED_FIELDS`: the engine walker's required fields per `slot/kind`, in its order.
fn required_fields(slot_kind: &str) -> Option<&'static [&'static str]> {
    Some(match slot_kind {
        "persister/Batch" => &["sink", "targetShape", "ingestMode"],
        "persister/Streaming" => &["sink"],
        "sink/Relational" => &["database"],
        "sink/ObjectStorage" => &["binding"],
        "targetShape/Flat" => &["modelClass", "targetName"],
        "targetShape/MultiFlat" => &["modelClass", "transactionScope", "parts"],
        "parts/__part__" => &["modelProperty", "targetName"],
        "deduplicationStrategy/MaxVersion" => &["versionField"],
        "deduplicationStrategy/DuplicateCount" => &["duplicateCountName"],
        "ingestMode/NontemporalSnapshot" => &["auditing"],
        "ingestMode/UnitemporalSnapshot" => &["transactionMilestoning"],
        "ingestMode/BitemporalSnapshot" => &["transactionMilestoning", "validityMilestoning"],
        "ingestMode/NontemporalDelta" => &["mergeStrategy", "auditing"],
        "ingestMode/UnitemporalDelta" => &["mergeStrategy", "transactionMilestoning"],
        "ingestMode/BitemporalDelta" => &["mergeStrategy", "transactionMilestoning", "validityMilestoning"],
        "ingestMode/AppendOnly" => &["auditing", "filterDuplicates"],
        "mergeStrategy/DeleteIndicator" => &["deleteField", "deleteValues"],
        "auditing/DateTime" => &["dateTimeName"],
        "transactionMilestoning/BatchId" => &["batchIdInName", "batchIdOutName"],
        "transactionMilestoning/DateTime" => &["dateTimeInName", "dateTimeOutName"],
        "transactionMilestoning/BatchIdAndDateTime" => {
            &["batchIdInName", "batchIdOutName", "dateTimeInName", "dateTimeOutName"]
        }
        "derivation/SourceSpecifiesInDateTime" => &["sourceDateTimeInField"],
        "derivation/SourceSpecifiesInAndOutDateTime" => &["sourceDateTimeInField", "sourceDateTimeOutField"],
        "validityMilestoning/DateTime" => &["dateTimeFromName", "dateTimeThruName", "derivation"],
        "derivation/SourceSpecifiesFromDateTime" => &["sourceDateTimeFromField"],
        "derivation/SourceSpecifiesFromAndThruDateTime" => &["sourceDateTimeFromField", "sourceDateTimeThruField"],
        "notifyees/Email" => &["address"],
        "notifyees/PagerDuty" => &["url"],
        "serviceOutput/TDS" => &["keys"],
        "target/Relational" => &["table"],
        "temporality/None" => &["updatesHandling"],
        _ => return None,
    })
}

/// `PERMITTED_FIELDS`: the keys the engine grammar permits per `slot/kind`, where a wrong-kind key corrupts the JSON.
fn permitted_fields(slot_kind: &str) -> Option<&'static [&'static str]> {
    Some(match slot_kind {
        "persister/Streaming" => &["sink"],
        "targetShape/Flat" => &["modelClass", "targetName", "partitionFields", "deduplicationStrategy"],
        "transactionMilestoning/BatchId" => &["batchIdInName", "batchIdOutName"],
        "datasetType/Snapshot" => &["partitioning"],
        "serviceOutput/TDS" => &["keys", "deduplication", "datasetType"],
        "temporality/None" => &["auditing", "updatesHandling"],
        "temporality/Unitemporal" => &["processingDimension"],
        "temporality/Bitemporal" => &["processingDimension", "sourceDerivedDimension"],
        "sourceFields/Start" => &["startField"],
        "sourceFields/StartAndEnd" => &["startField", "endField"],
        _ => return None,
    })
}

/// `CHILD_KINDS`: the derivation kinds each milestoning arm admits, keyed `parentSlot/childKey`.
fn child_kinds(slot_key: &str) -> Option<&'static [&'static str]> {
    Some(match slot_key {
        "transactionMilestoning/derivation" => &["SourceSpecifiesInDateTime", "SourceSpecifiesInAndOutDateTime"],
        "validityMilestoning/derivation" => &["SourceSpecifiesFromDateTime", "SourceSpecifiesFromAndThruDateTime"],
        _ => return None,
    })
}

/// `SLOT_KINDS`: the slots whose kind set is closed in the grammar.
fn slot_kinds(slot: &str) -> Option<&'static [&'static str]> {
    Some(match slot {
        "validityMilestoning" => &["DateTime"],
        "target" => &["Relational", "__empty__"],
        _ => return None,
    })
}

fn listed(list: &[&str], v: &[u16]) -> bool {
    list.iter().any(|s| eq(v, s))
}

/// `slot + "/" + x` as a lookup key (a name with a lone surrogate matches no table entry, as in Java).
fn joined(a: &str, sep: &str, b: &[u16]) -> String {
    format!("{a}{sep}{}", String::from_utf16_lossy(b))
}

/// `validateNode(c, slot, node)`: the engine walker's cardinality over the node tree, every refusal at the node's own
/// span start -- the closed slot kinds, the permitted keys, the child kinds, then each required field in order (once,
/// present, its subtree walked), every other key once, their subtrees walked.
fn validate_node(slot: &str, n: &PPersistenceNode) -> R<()> {
    let Some(span) = n.source_information.as_deref() else {
        // NO_DEDUPLICATION or KEYLESS_TARGET
        return Ok(());
    };
    let (line, col) = (span.start_line, span.start_column);
    let unexpected = || JavaError::parse(JStr::from("Unexpected token"), line, col);
    if let Some(kinds) = slot_kinds(slot)
        && !listed(kinds, &n.kind.0)
    {
        return Err(unexpected());
    }
    let slot_kind = joined(slot, "/", &n.kind.0);
    if let Some(permitted) = permitted_fields(&slot_kind) {
        for e in &n.entries {
            if !listed(permitted, entry_key(e)) {
                return Err(unexpected());
            }
        }
    }
    for e in &n.entries {
        if let PPersistenceEntry::PPersistenceEntryNode(nd) = e
            && let Some(kinds) = child_kinds(&joined(slot, "/", &nd.key.0))
            && !listed(kinds, &nd.node.kind.0)
        {
            return Err(unexpected());
        }
    }
    let mut walked: HashSet<Vec<u16>> = HashSet::new();
    if let Some(ordered) = required_fields(&slot_kind) {
        for r in ordered {
            walked.insert(u(r));
            let count = n.entries.iter().filter(|e| eq(entry_key(e), r)).count();
            if count > 1 {
                return Err(JavaError::parse(
                    jcat!("Field '", *r, "' should be specified only once"),
                    line,
                    col,
                ));
            }
            if count == 0 {
                return Err(JavaError::parse(jcat!("Field '", *r, "' is required"), line, col));
            }
            for e in &n.entries {
                if eq(entry_key(e), r) {
                    recurse_entry(e)?;
                }
            }
        }
    }
    let mut seen: HashSet<&[u16]> = HashSet::new();
    for e in &n.entries {
        let k = entry_key(e);
        if !walked.contains(k) && !seen.insert(k) {
            return Err(JavaError::parse(
                jcat!("Field '", k, "' should be specified only once"),
                line,
                col,
            ));
        }
    }
    for e in &n.entries {
        if !walked.contains(entry_key(e)) {
            recurse_entry(e)?;
        }
    }
    Ok(())
}

fn recurse_entry(e: &PPersistenceEntry) -> R<()> {
    match e {
        PPersistenceEntry::PPersistenceEntryNode(nd) => validate_node(&String::from_utf16_lossy(&nd.key.0), &nd.node),
        PPersistenceEntry::PPersistenceEntryNodeList(nl) => {
            let slot = String::from_utf16_lossy(&nl.key.0);
            for n in &nl.nodes {
                validate_node(&slot, n)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `SPANLESS_OUTSIDE_TARGET`.
const SPANLESS_OUTSIDE_TARGET: &[&str] = &[
    "auditing:None",
    "mergeStrategy:NoDeletes",
    "mergeStrategy:DeleteIndicator",
];

/// `wireSpans(node, slot, target)`: the spans the engine's JSON carries and no others -- the spanless kinds outside a
/// persistence target, an entry-less deduplication strategy -- applied after validation.
fn wire_spans(n: PPersistenceNode, slot: &[u16], target: bool) -> PPersistenceNode {
    let entries_empty = n.entries.is_empty();
    let entries = n
        .entries
        .into_iter()
        .map(|e| match e {
            PPersistenceEntry::PPersistenceEntryNode(nd) => {
                let PPersistenceEntryNode { key, node: inner } = *nd;
                let inner = wire_spans(*inner, &key.0, target);
                PPersistenceEntry::PPersistenceEntryNode(Box::new(PPersistenceEntryNode {
                    key,
                    node: Box::new(inner),
                }))
            }
            PPersistenceEntry::PPersistenceEntryNodeList(nl) => {
                let PPersistenceEntryNodeList { key, nodes } = *nl;
                let nodes = nodes
                    .into_iter()
                    .map(|x| Box::new(wire_spans(*x, &key.0, target)))
                    .collect();
                PPersistenceEntry::PPersistenceEntryNodeList(Box::new(PPersistenceEntryNodeList { key, nodes }))
            }
            other => other,
        })
        .collect();
    let slot_kind = joined(&String::from_utf16_lossy(slot), ":", &n.kind.0);
    let spanless = (!target && SPANLESS_OUTSIDE_TARGET.contains(&slot_kind.as_str()))
        || (eq(slot, "deduplicationStrategy") && entries_empty);
    let span = if spanless { None } else { n.source_information };
    node(n.kind, n.head_path, entries, span)
}

/// `contentAnchoredIslandNode`: `Kind #{ entries }#`, its span from the content (the line after `#{` at the earliest)
/// to one past the `}#` -- the persistence target walker's quirk.
fn content_anchored_island_node<'s>(c: &mut impl Cursor<'s>) -> R<PPersistenceNode> {
    let kind = c.parse_identifier()?;
    if !is_island(c.peek()) {
        return Err(c.error(jcat!("'", kind, "' needs an island body")));
    }
    let emb_start = scan_island(c)?;
    let emb = c.reconstruct_text(emb_start, c.pos());
    let mut entries = Vec::new();
    let island = tokenize(&emb);
    let off = (
        start_line(c.tokens(), emb_start)? - 1,
        start_column(c.tokens(), emb_start)? - 1,
    );
    island_entries(&island, off, c.dialect(), &mut entries)?;
    let cs = c.span_of(emb_start, emb_start);
    let open_line = start_line(c.tokens(), emb_start - 1)?;
    let end_tok = c.pos();
    c.expect(T::ISLAND_END)?;
    let es = c.span_of(end_tok, end_tok);
    let span = SourceInfo {
        source_id: JStr::default(),
        start_line: (open_line + 1).max(cs.start_line),
        start_column: cs.start_column,
        end_line: es.end_line,
        end_column: es.end_column + 1,
    };
    Ok(node(JStr(kind), None, entries, Some(Box::new(span))))
}

/// Persistence's island re-lex cursor (`OffsetCursor`): its spans shifted by the island's offsets, its errors left in
/// the island's own coordinates.
fn island_cursor<'t, 's>(
    ts: &'t TokenStream<'s>,
    off: (i32, i32),
    dialect: crate::doors::Dialect,
) -> IslandCursor<'t, 's> {
    IslandCursor {
        tokens: ts,
        pos: 0,
        dialect,
        line_offset: off.0,
        col_offset: off.1,
        runtime: false,
    }
}

/// An island's entries, read on a cursor of its own: legend-engine's walk reads them, so the reading for a later syntax
/// error passes them by.
fn island_entries(
    island: &TokenStream<'_>,
    off: (i32, i32),
    level: crate::doors::Dialect,
    entries: &mut Vec<PPersistenceEntry>,
) -> R<()> {
    match parse_entries(&mut island_cursor(island, off, level), off, entries, None) {
        Err(_) if level.reads_for_syntax() => Ok(()),
        answer => answer,
    }
}

/// `parseNode(c)`: a path-headed node (`#/C/p# { ... }`), `Kind { entries }`, `Kind #{ entries }#` (re-lexed, the
/// cursor's own offsets `off` composed into the island's) or a bare `Kind`.
fn parse_node<'s>(c: &mut impl Cursor<'s>, off: (i32, i32)) -> R<PPersistenceNode> {
    let s = c.pos();
    if c.peek() == T::PATH_LITERAL {
        let head_path = one_token_spec(c, c.pos())?;
        c.advance()?;
        let mut path_entries = Vec::new();
        c.expect(T::BRACE_OPEN)?;
        parse_entries(c, off, &mut path_entries, Some(T::BRACE_CLOSE))?;
        c.expect(T::BRACE_CLOSE)?;
        return Ok(node(
            JStr::from("#path"),
            Some(head_path),
            path_entries,
            c.span(s, c.pos() - 1),
        ));
    }
    let kind = c.parse_identifier()?;
    let mut entries = Vec::new();
    if c.match_tok(T::BRACE_OPEN)? {
        parse_entries(c, off, &mut entries, Some(T::BRACE_CLOSE))?;
        c.expect(T::BRACE_CLOSE)?;
    } else if is_island(c.peek()) {
        let open = c.pos();
        let emb_start = scan_island(c)?;
        if emb_start >= c.tokens().count() && !admits_island(&kind) {
            // the engine's grammar admits no island after this kind: it refuses the `#{`, in the text's own place
            let ts = c.tokens();
            let line = ts.start_line(open) as i32;
            let column = ts.start_column(open) as i32 + if line == 1 { off.1 } else { 0 };
            return Err(
                JavaError::parse(jcat!("Unexpected token '", ts.text(open), "'"), line + off.0, column).as_refusal(),
            );
        }
        let emb = c.reconstruct_text(emb_start, c.pos());
        let base_line = start_line(c.tokens(), emb_start)? - 1 + off.0;
        let base_col = start_column(c.tokens(), emb_start)? - 1 + off.1;
        let island = tokenize(&emb);
        let ic_off = (base_line, base_col);
        c.expect(T::ISLAND_END)?;
        island_entries(&island, ic_off, c.dialect(), &mut entries)?;
    } else {
        return Ok(node(JStr(kind), None, entries, c.span(s, s)));
    }
    Ok(node(JStr(kind), None, entries, c.span(s, c.pos() - 1)))
}

/// The keys of the engine's persistence grammars (its lexer's keywords; ruling 18 of 2026-11-09).
const PERSISTENCE_KEYS: &[&str] = &[
    "actionIndicator",
    "address",
    "appendStrategy",
    "asserts",
    "auditing",
    "batchIdIn",
    "batchIdInName",
    "batchIdOut",
    "batchIdOutName",
    "binding",
    "connection",
    "contentType",
    "data",
    "dataProcessingUnits",
    "database",
    "datasetType",
    "dateTimeEnd",
    "dateTimeFromName",
    "dateTimeIn",
    "dateTimeInName",
    "dateTimeName",
    "dateTimeOut",
    "dateTimeOutName",
    "dateTimeStart",
    "dateTimeThruName",
    "deduplication",
    "deduplicationStrategy",
    "deleteField",
    "deleteValues",
    "derivation",
    "doc",
    "duplicateCountName",
    "emptyDatasetHandling",
    "endField",
    "expected",
    "filterDuplicates",
    "graphFetchPath",
    "ingestMode",
    "isTestDataFromServiceOutput",
    "keys",
    "mergeStrategy",
    "modelClass",
    "modelProperty",
    "notifier",
    "notifyees",
    "partitionFields",
    "partitioning",
    "parts",
    "persistence",
    "persister",
    "platform",
    "processingDimension",
    "service",
    "serviceOutput",
    "serviceOutputTargets",
    "serviceParameters",
    "sink",
    "sinkConnection",
    "sourceDateTimeFromField",
    "sourceDateTimeInField",
    "sourceDateTimeOutField",
    "sourceDateTimeThruField",
    "sourceDerivedDimension",
    "sourceFields",
    "startField",
    "table",
    "target",
    "targetName",
    "targetShape",
    "temporality",
    "testBatches",
    "tests",
    "transactionMilestoning",
    "transactionScope",
    "trigger",
    "updatesHandling",
    "url",
    "validityMilestoning",
    "versionField",
];

/// The kinds the engine's grammar writes with an island: a test's data and its assertions.
fn admits_island(kind: &[u16]) -> bool {
    eq(kind, "ExternalFormat") || eq(kind, "Reference") || starts_with(kind, "EqualTo")
}

/// `withFlatDefaults`: a flat target's (and each multi-flat part's) entries with the engine protocol's defaults where
/// unwritten -- no deduplication (spanless), no partition fields.
fn with_flat_defaults(entries: Vec<PPersistenceEntry>) -> Vec<PPersistenceEntry> {
    let has = |k: &str| entries.iter().any(|e| eq(entry_key(e), k));
    let (dedup, partitions) = (has("deduplicationStrategy"), has("partitionFields"));
    let mut out = entries;
    if !dedup {
        out.push(node_entry("deduplicationStrategy", no_deduplication()));
    }
    if !partitions {
        out.push(PPersistenceEntry::PPersistenceEntryStrings(Box::new(
            PPersistenceEntryStrings {
                key: JStr::from("partitionFields"),
                values: Vec::new(),
            },
        )));
    }
    out
}

fn scalar(key: Vec<u16>, value: Vec<u16>, quoted: bool) -> PPersistenceEntry {
    PPersistenceEntry::PPersistenceEntryScalar(Box::new(PPersistenceEntryScalar {
        key: JStr(key),
        value: JStr(value),
        quoted,
    }))
}

/// `parseEntries(c, out, close)`: `key: value` entries to `close` (or the stream's end) -- a string, a boolean, a list
/// (of keyless parts, of path literals, of strings and identifiers), a path literal, an integer, or a qualified head:
/// a node when a body follows, a pointer for `database` and `binding`, a CamelCase leaf node, else a string.
fn parse_entries<'s>(
    c: &mut impl Cursor<'s>,
    off: (i32, i32),
    out: &mut Vec<PPersistenceEntry>,
    close: Option<TokenType>,
) -> R<()> {
    while !c.at_end() && close.is_none_or(|t| c.peek() != t) {
        let key_start = c.pos();
        let key = c.parse_identifier()?;
        if close.is_some() && c.dialect().refuses_lite_extensions() && !PERSISTENCE_KEYS.iter().any(|k| eq(&key, k)) {
            c.set_pos(key_start);
            return Err(c.unexpected());
        }
        c.expect(T::COLON)?;
        match c.peek() {
            T::STRING => {
                let v = string_value(c)?;
                out.push(scalar(key, v, true));
                c.expect(T::SEMI_COLON)?;
            }
            T::TRUE | T::FALSE => {
                let v = c.peek() == T::TRUE;
                c.advance()?;
                out.push(scalar(key, u(if v { "true" } else { "false" }), false));
                c.expect(T::SEMI_COLON)?;
            }
            T::BRACKET_OPEN => {
                c.advance()?;
                if c.peek() == T::BRACE_OPEN {
                    let mut nodes = Vec::new();
                    while c.peek() != T::BRACKET_CLOSE {
                        let ps = c.pos();
                        c.expect(T::BRACE_OPEN)?;
                        let mut pe = Vec::new();
                        parse_entries(c, off, &mut pe, Some(T::BRACE_CLOSE))?;
                        c.expect(T::BRACE_CLOSE)?;
                        nodes.push(Box::new(node(
                            JStr::from("__part__"),
                            None,
                            with_flat_defaults(pe),
                            c.span(ps, c.pos() - 1),
                        )));
                        if !c.match_list_comma()? {
                            break;
                        }
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                    c.match_tok(T::SEMI_COLON)?;
                    out.push(PPersistenceEntry::PPersistenceEntryNodeList(Box::new(
                        PPersistenceEntryNodeList { key: JStr(key), nodes },
                    )));
                    continue;
                }
                if c.peek() == T::PATH_LITERAL {
                    let mut specs = Vec::new();
                    while c.peek() != T::BRACKET_CLOSE {
                        specs.push(one_token_spec(c, c.pos())?);
                        c.expect(T::PATH_LITERAL)?;
                        if !c.match_list_comma()? {
                            break;
                        }
                    }
                    c.expect(T::BRACKET_CLOSE)?;
                    c.match_tok(T::SEMI_COLON)?;
                    out.push(PPersistenceEntry::PPersistenceEntryPathList(Box::new(
                        PPersistenceEntryPathList { key: JStr(key), specs },
                    )));
                    continue;
                }
                let mut vals = Vec::new();
                while c.peek() != T::BRACKET_CLOSE {
                    if c.peek() == T::STRING {
                        vals.push(JStr(string_value(c)?));
                    } else if eq(&key, "deleteValues") {
                        return Err(c.error(jcat!("Unexpected token '", c.safe_text(), "'")));
                    } else {
                        vals.push(JStr(c.parse_identifier()?));
                    }
                    if !c.match_list_comma()? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
                c.match_tok(T::SEMI_COLON)?;
                out.push(PPersistenceEntry::PPersistenceEntryStrings(Box::new(
                    PPersistenceEntryStrings {
                        key: JStr(key),
                        values: vals,
                    },
                )));
            }
            T::PATH_LITERAL => {
                let spec = one_token_spec(c, c.pos())?;
                out.push(PPersistenceEntry::PPersistenceEntryPathValue(Box::new(
                    PPersistenceEntryPathValue { key: JStr(key), spec },
                )));
                c.advance()?;
                c.expect(T::SEMI_COLON)?;
            }
            T::INTEGER => {
                out.push(scalar(key, c.text().to_vec(), false));
                c.advance()?;
                c.expect(T::SEMI_COLON)?;
            }
            _ => {
                let mut head = unquote_path_iae(&qn_admitting_booleans(c)?)?;
                while c.peek() == T::DOT {
                    c.advance()?;
                    head.push(u16::from(b'.'));
                    head.extend(c.parse_identifier()?);
                }
                if c.peek() == T::BRACE_OPEN || is_island(c.peek()) {
                    c.set_pos(key_start);
                    c.parse_identifier()?;
                    c.expect(T::COLON)?;
                    let mut n = parse_node(c, off)?;
                    if eq(&key, "targetShape") && eq(&n.kind.0, "Flat") {
                        n = node(n.kind, n.head_path, with_flat_defaults(n.entries), n.source_information);
                    }
                    out.push(PPersistenceEntry::PPersistenceEntryNode(Box::new(
                        PPersistenceEntryNode {
                            key: JStr(key),
                            node: Box::new(n),
                        },
                    )));
                    c.match_tok(T::SEMI_COLON)?;
                } else if eq(&key, "database") || eq(&key, "binding") {
                    c.expect(T::SEMI_COLON)?;
                    out.push(PPersistenceEntry::PPersistenceEntryPointer(Box::new(
                        PPersistenceEntryPointer {
                            key: JStr(key),
                            path: JStr(head),
                            source_information: c.span(key_start, c.pos() - 1),
                        },
                    )));
                } else if !head.is_empty()
                    && is_upper_case(head[0])
                    && head.iter().any(|&ch| is_lower_case(ch))
                    && index_of_units(&head, &u("::"), 0).is_none()
                    && !eq(&key, "table")
                    && !eq(&key, "targetName")
                {
                    c.expect(T::SEMI_COLON)?;
                    let ks = c.span_of(key_start + 2, key_start + 2);
                    let leaf = SourceInfo {
                        end_column: ks.end_column + 1,
                        ..ks
                    };
                    out.push(PPersistenceEntry::PPersistenceEntryNode(Box::new(
                        PPersistenceEntryNode {
                            key: JStr(key),
                            node: Box::new(node(JStr(head), None, Vec::new(), Some(Box::new(leaf)))),
                        },
                    )));
                } else {
                    c.expect(T::SEMI_COLON)?;
                    out.push(scalar(key, head, true));
                }
            }
        }
    }
    Ok(())
}

/// `parseOutputTargets`: `[ ServiceOutput -> Target, ... ]`, at least one; a target `{ }` keyless, else an island.
fn parse_output_targets<'s>(c: &mut impl Cursor<'s>) -> R<Vec<PServiceOutputTarget>> {
    let mut out = Vec::new();
    c.expect(T::BRACKET_OPEN)?;
    if c.peek() == T::BRACKET_CLOSE {
        return Err(c.error_str("Unexpected token ']'"));
    }
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let s = c.pos();
        let service_output = parse_node(c, (0, 0))?;
        c.expect(T::ARROW)?;
        let target = if c.peek() == T::BRACE_OPEN {
            c.expect(T::BRACE_OPEN)?;
            c.expect(T::BRACE_CLOSE)?;
            keyless_target()
        } else {
            content_anchored_island_node(c)?
        };
        out.push(PServiceOutputTarget {
            service_output: Box::new(service_output),
            persistence_target: Box::new(target),
            source_information: c.span(s, c.pos() - 1),
        });
        c.list_comma()?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    Ok(out)
}

/// `parseNotifier`: `{ notifyees: [ node, ... ]; }`, `notifyees` required and once (at the `notifier` key), each node
/// validated as it is read; the span from the key.
fn parse_notifier<'s>(c: &mut impl Cursor<'s>, key_start: usize) -> R<PPersistenceNotifier> {
    c.expect(T::BRACE_OPEN)?;
    let mut notifyees = Vec::new();
    let mut spelled = false;
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !eq(&key, "notifyees") {
            return Err(c.error(jcat!("unknown notifier key '", key, "'")));
        }
        if spelled {
            return Err(throw_at(
                c.tokens(),
                key_start,
                JStr::from("Field 'notifyees' should be specified only once"),
            ));
        }
        spelled = true;
        c.expect(T::BRACKET_OPEN)?;
        while c.peek() != T::BRACKET_CLOSE {
            let n = parse_node(c, (0, 0))?;
            validate_node("notifyees", &n)?;
            notifyees.push(Box::new(n));
            if !c.match_list_comma()? {
                break;
            }
        }
        c.expect(T::BRACKET_CLOSE)?;
        c.expect(T::SEMI_COLON)?;
    }
    c.expect(T::BRACE_CLOSE)?;
    if !spelled {
        return Err(throw_at(
            c.tokens(),
            key_start,
            JStr::from("Field 'notifyees' is required"),
        ));
    }
    Ok(PPersistenceNotifier {
        notifyees,
        source_information: c.span(key_start, c.pos() - 1),
    })
}

/// `parseTests`: `[ id: { testBatches: [...] isTestDataFromServiceOutput: b; graphFetchPath: #/..#; }, ... ]`, each key
/// once (at the cursor), `testBatches` required.
fn parse_tests<'s>(c: &mut impl Cursor<'s>) -> R<Vec<Box<PPersistenceTest>>> {
    let mut out = Vec::new();
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let s = c.pos();
        let id = c.parse_identifier()?;
        c.expect(T::COLON)?;
        c.expect(T::BRACE_OPEN)?;
        let mut batches = Vec::new();
        let mut from_service_output = false;
        let mut graph_fetch_path = None;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        let mut spelled = false;
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if !seen.insert(key.clone()) {
                return Err(c.error(jcat!("Field '", key, "' should be specified only once")));
            }
            if eq(&key, "testBatches") {
                spelled = true;
                parse_test_batches(c, &mut batches)?;
            } else if eq(&key, "isTestDataFromServiceOutput") {
                from_service_output = c.match_tok(T::TRUE)?;
                if !from_service_output {
                    c.expect(T::FALSE)?;
                }
                c.expect(T::SEMI_COLON)?;
            } else if eq(&key, "graphFetchPath") {
                graph_fetch_path = Some(one_token_spec(c, c.pos())?);
                c.expect(T::PATH_LITERAL)?;
                c.expect(T::SEMI_COLON)?;
            } else {
                return Err(c.error(jcat!("unknown test key '", key, "'")));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        if !spelled {
            return Err(throw_at(c.tokens(), s, JStr::from("Field 'testBatches' is required")));
        }
        out.push(Box::new(PPersistenceTest {
            id: JStr(id),
            test_batches: Some(batches),
            is_test_data_from_service_output: Some(from_service_output),
            graph_fetch_path,
            source_information: c.span(s, c.pos() - 1),
        }));
        c.list_comma()?;
    }
    c.expect(T::BRACKET_CLOSE)?;
    Ok(out)
}

/// `parseTestBatches`: `[ id: { data: { connection: { node } } asserts: [ id: node, ... ] }, ... ]`, each key once (at
/// the cursor); `data` and `asserts` required; an assertion's span from its value, its node's own dropped.
fn parse_test_batches<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PPersistenceTestBatch>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while c.peek() != T::BRACKET_CLOSE && !c.at_end() {
        let s = c.pos();
        let id = c.parse_identifier()?;
        c.expect(T::COLON)?;
        c.expect(T::BRACE_OPEN)?;
        let mut conn = None;
        let mut asserts = Vec::new();
        let mut spelled = false;
        let mut seen: HashSet<Vec<u16>> = HashSet::new();
        while !c.at_end() && c.peek() != T::BRACE_CLOSE {
            let key_start = c.pos();
            let key = c.parse_identifier()?;
            c.expect(T::COLON)?;
            if !seen.insert(key.clone()) {
                return Err(c.error(jcat!("Field '", key, "' should be specified only once")));
            }
            if eq(&key, "data") {
                c.expect(T::BRACE_OPEN)?;
                let conn_key = c.pos();
                let ck = c.parse_identifier()?;
                if !eq(&ck, "connection") {
                    return Err(c.error(jcat!("unknown data key '", ck, "'")));
                }
                c.expect(T::COLON)?;
                c.expect(T::BRACE_OPEN)?;
                let data = parse_node(c, (0, 0))?;
                c.expect(T::BRACE_CLOSE)?;
                let conn_span = c.span(conn_key, c.pos() - 1);
                c.expect(T::BRACE_CLOSE)?;
                conn = Some((data, conn_span, c.span(key_start, c.pos() - 1)));
            } else if eq(&key, "asserts") {
                spelled = true;
                c.expect(T::BRACKET_OPEN)?;
                while c.peek() != T::BRACKET_CLOSE {
                    let aid = c.parse_identifier()?;
                    c.expect(T::COLON)?;
                    let a_s = c.pos();
                    if c.dialect().holds_to_the_engines_walk() {
                        refuse_what_equal_to_json_reads(c)?;
                    }
                    let an = parse_node(c, (0, 0))?;
                    asserts.push(Box::new(PPersistenceAssert {
                        id: JStr(aid),
                        assertion: Box::new(node(an.kind, an.head_path, an.entries, None)),
                        source_information: c.span(a_s, c.pos() - 1),
                    }));
                    if !c.match_list_comma()? {
                        break;
                    }
                }
                c.expect(T::BRACKET_CLOSE)?;
            } else {
                return Err(c.error(jcat!("unknown testBatch key '", key, "'")));
            }
        }
        c.expect(T::BRACE_CLOSE)?;
        let Some((data, conn_span, data_span)) = conn else {
            return Err(throw_at(c.tokens(), s, JStr::from("Field 'data' is required")));
        };
        if !spelled {
            return Err(throw_at(c.tokens(), s, JStr::from("Field 'asserts' is required")));
        }
        out.push(Box::new(PPersistenceTestBatch {
            id: JStr(id),
            connection_data: Some(Box::new(data)),
            connection_span: conn_span,
            data_span,
            asserts: Some(asserts),
            source_information: c.span(s, c.pos() - 1),
            has_test_data: true,
        }));
        c.list_comma()?;
    }
    c.expect(T::BRACKET_CLOSE)
}

/// What legend-engine's grammar reads of an `EqualToJson` assertion's island: `expected: <identifier> #{ ... }#;` and nothing
/// after it. The emitter has no wire for the other entries the generic walker takes (a list, a path, a pointer); read at an
/// engine-JSON door after the emitter refused them, each is refused where the engine's parser stops, lite's wording (the
/// user's ruling 15 of 2026-11-09, docs/rust-port/PHASE2.md section 6; PARITY row 74).
fn refuse_what_equal_to_json_reads<'s>(c: &mut impl Cursor<'s>) -> R<()> {
    let head = c.pos();
    if c.at_end() || !eq(c.text(), "EqualToJson") {
        return Ok(());
    }
    if c.peek_at(1) == T::BRACE_OPEN {
        return Err(refused_at(c.tokens(), head + 1, JStr::from("Unexpected token '{'")));
    }
    if !is_island(c.peek_at(1)) {
        return Ok(());
    }
    c.advance()?;
    let emb_start = scan_island(c)?;
    let at_end = c.at_end();
    let emb = c.reconstruct_text(emb_start, c.pos());
    let island = tokenize(&emb);
    let (line_off, col_off) = if at_end || emb_start >= c.tokens().count() {
        (0, 0)
    } else {
        (
            start_line(c.tokens(), emb_start)? - 1,
            start_column(c.tokens(), emb_start)? - 1,
        )
    };
    c.set_pos(head);
    let refused = |i: usize| {
        let line = island.start_line(i) as i32;
        let column = island.start_column(i) as i32 + if line == 1 { col_off } else { 0 };
        JavaError::parse(
            jcat!("Unexpected token '", island.text(i), "'"),
            line + line_off,
            column,
        )
        .as_refusal()
    };
    let n = island.count();
    let ty = |i: usize| (i < n).then(|| island.ty(i));
    if n == 0 {
        return Ok(());
    }
    if !eq(island.text(0), "expected") {
        return Err(refused(0));
    }
    if n > 1 && ty(1) != Some(T::COLON) {
        return Err(refused(1));
    }
    if n > 2 && !is_identifier_token(island.ty(2)) {
        return Err(refused(2));
    }
    if n > 3 && !ty(3).is_some_and(is_island) {
        return Err(refused(3));
    }
    let mut i = 4;
    let mut depth = 0i64;
    while i < n {
        match island.ty(i) {
            T::ISLAND_START => depth += 1,
            T::ISLAND_END if depth == 0 => break,
            T::ISLAND_END => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    if i + 1 < n && island.ty(i + 1) != T::SEMI_COLON {
        return Err(refused(i + 1));
    }
    if i + 2 < n {
        return Err(refused(i + 2));
    }
    Ok(())
}

/// `parseContext`: `PersistenceContext <<dec>> qn { persistence: qn; platform: ...; serviceParameters: [...];
/// sinkConnection: ...; }`, each key once (at the declaration), `persistence` required.
fn parse_context<'s>(c: &mut impl Cursor<'s>) -> R<PPersistenceContext> {
    let decl_start = c.pos();
    c.advance()?;
    let dec = c.parse_decorations()?;
    let qn = unquote_path_iae(&qn_admitting_booleans(c)?)?;
    let (pkg, name) = split_last(&qn);
    c.expect(T::BRACE_OPEN)?;
    let mut persistence = None;
    let mut persistence_span = None;
    let mut platform = None;
    let mut params = Vec::new();
    let mut sink_connection = None;
    let mut seen: HashSet<Vec<u16>> = HashSet::new();
    while !c.at_end() && c.peek() != T::BRACE_CLOSE {
        let key_start = c.pos();
        let key = c.parse_identifier()?;
        c.expect(T::COLON)?;
        if !seen.insert(key.clone()) {
            return Err(throw_at(
                c.tokens(),
                decl_start,
                jcat!("Field '", key, "' should be specified only once"),
            ));
        }
        match String::from_utf16_lossy(&key).as_str() {
            "persistence" => {
                persistence = Some(JStr(unquote_path_iae(&qn_admitting_booleans(c)?)?));
                c.expect(T::SEMI_COLON)?;
                persistence_span = c.span(key_start, c.pos() - 1);
            }
            "platform" => platform = Some(Box::new(parse_platform(c)?)),
            "serviceParameters" => {
                parse_ctx_params(c, &mut params)?;
                c.match_tok(T::SEMI_COLON)?;
            }
            "sinkConnection" => {
                sink_connection = Some(if is_island(c.peek()) {
                    parse_connection_island(c)?
                } else {
                    let vs = c.pos();
                    let path = unquote_path_iae(&qn_admitting_booleans(c)?)?;
                    PConnectionValue::PConnectionPointer(Box::new(PConnectionPointer {
                        connection: JStr(path),
                        source_information: c.span(vs, c.pos() - 1),
                    }))
                });
                c.match_tok(T::SEMI_COLON)?;
            }
            _ => return Err(c.error(jcat!("unknown key '", key, "' inside PersistenceContext '", qn, "'"))),
        }
    }
    c.expect(T::BRACE_CLOSE)?;
    let Some(persistence) = persistence else {
        return Err(throw_at(
            c.tokens(),
            decl_start,
            JStr::from("Field 'persistence' is required"),
        ));
    };
    Ok(PPersistenceContext {
        pkg: JStr(pkg),
        name: JStr(name),
        stereotypes: dec.stereotypes,
        tagged_values: dec.tagged_values,
        persistence,
        persistence_span,
        platform,
        service_parameters: params,
        sink_connection,
        source_information: c.span(decl_start, c.pos() - 1),
    })
}

/// A context's `platform:` value: `Default;` (its span the kind word), or `Kind #{ entries }#` (its span from the
/// content, the line after `#{` at the earliest, to one past the `}#`); `AwsGlue`'s `dataProcessingUnits` required and
/// once, refused where the cloud extension's own parse of the island would place it.
fn parse_platform<'s>(c: &mut impl Cursor<'s>) -> R<PPersistenceNode> {
    let platform_tok = c.pos();
    let kind = c.parse_identifier()?;
    let aws = eq(&kind, "AwsGlue");
    let syntax = c.dialect().reads_for_syntax();
    if c.dialect().holds_to_the_engines_walk() && is_island(c.peek()) {
        refuse_a_one_character_island(c, &kind)?;
    }
    if !eq(&kind, "Default") && !aws && !syntax {
        return Err(c.error(jcat!("Unsupported persistence platform type '", kind, "'")));
    }
    if c.peek() == T::SEMI_COLON {
        if aws && !syntax {
            return Err(c.error_str("Persistence platform 'AwsGlue' must have a non-empty body"));
        }
        let kind_tok = c.pos() - 1;
        c.advance()?;
        return Ok(node(JStr(kind), None, Vec::new(), c.span(kind_tok, kind_tok)));
    }
    if !is_island(c.peek()) {
        return Err(c.error(jcat!("platform '", kind, "' needs an island body")));
    }
    let open = c.pos();
    let emb_start = scan_island(c)?;
    if aws && c.dialect().holds_to_the_engines_walk() {
        refuse_what_aws_glue_reads(c, open, platform_tok)?;
    }
    let emb = c.reconstruct_text(emb_start, c.pos());
    let mut entries = Vec::new();
    let island = tokenize(&emb);
    let off = (
        start_line(c.tokens(), emb_start)? - 1,
        start_column(c.tokens(), emb_start)? - 1,
    );
    island_entries(&island, off, c.dialect(), &mut entries)?;
    // deliberately stricter than the engine, which reads the body and writes a default platform: the emitter has no wire
    // for one (the user's ruling 11 of 2026-11-09, docs/rust-port/PHASE2.md section 6; PARITY row 73, register S158)
    if !aws && !entries.is_empty() && c.dialect().answers_engine_json() {
        return Err(refused_at(
            c.tokens(),
            platform_tok,
            JStr::from("Persistence platform 'Default' must not have a body"),
        ));
    }
    if aws {
        let (a_line, a_col) = if island.count() > 0 {
            let l = island.start_line(0) as i32;
            let a_line = l + start_line(c.tokens(), emb_start)? - 1;
            let a_col = if l == 1 {
                island.start_column(0) as i32 + start_column(c.tokens(), emb_start)? - 1
            } else {
                island.start_column(0) as i32
            };
            (a_line, a_col)
        } else {
            (start_line(c.tokens(), c.pos())?, 1)
        };
        let dpu = entries
            .iter()
            .filter(|e| eq(entry_key(e), "dataProcessingUnits"))
            .count();
        if dpu == 0 {
            return Err(JavaError::parse(
                JStr::from("Field 'dataProcessingUnits' is required"),
                a_line,
                a_col,
            ));
        }
        if dpu > 1 {
            return Err(JavaError::parse(
                JStr::from("Field 'dataProcessingUnits' should be specified only once"),
                a_line,
                a_col,
            ));
        }
    }
    let cs = c.span_of(emb_start, emb_start);
    let open_line = start_line(c.tokens(), emb_start - 1)?;
    let end_tok = c.pos();
    c.expect(T::ISLAND_END)?;
    let es = c.span_of(end_tok, end_tok);
    let span = SourceInfo {
        source_id: JStr::default(),
        start_line: (open_line + 1).max(cs.start_line),
        start_column: cs.start_column,
        end_line: es.end_line,
        end_column: es.end_column + 1,
    };
    let platform = node(JStr(kind), None, entries, Some(Box::new(span)));
    c.match_tok(T::SEMI_COLON)?;
    Ok(platform)
}

/// The text between an island's `#{` at `open` and its `}#`, the cursor on it; none when the input ends first.
fn island_text<'s>(c: &impl Cursor<'s>, open: usize) -> Option<Vec<u16>> {
    if c.at_end() {
        return None;
    }
    let ts = c.tokens();
    Some(ts.source()[ts.end(open)..ts.start(c.pos())].to_vec())
}

/// legend-engine's walker cuts two characters off a platform island's text before it looks at the platform's kind (its
/// rule has left the `}#` out already): one character is its `substring` failing, which its section catch-all answers
/// (the user's ruling 16 of 2026-11-09, docs/rust-port/PHASE2.md section 6; PARITY row 73). Lite's wording.
fn refuse_a_one_character_island<'s>(c: &mut impl Cursor<'s>, kind: &[u16]) -> R<()> {
    let open = c.pos();
    scan_island(c)?;
    let text = island_text(c, open);
    c.set_pos(open);
    if text.is_some_and(|t| t.len() == 1) {
        return Err(c.section_refusal(jcat!(
            "Persistence platform '",
            kind,
            "' must not have a one-character body"
        )));
    }
    Ok(())
}

/// What legend-engine's cloud grammar reads of an `AwsGlue` island, its walker having cut the text's last two characters:
/// `(dataProcessingUnits: <integer>;)*` to the end (the user's ruling 16 of 2026-11-09, docs/rust-port/PHASE2.md section 6;
/// PARITY row 73). Refused where its parser stops, in the document's place: the text's first line is shifted past the
/// `#{`. A `;` the cut left out is ANTLR's missing token, which the engine places a column before the end of the text.
fn refuse_what_aws_glue_reads<'s>(c: &impl Cursor<'s>, open: usize, platform_tok: usize) -> R<()> {
    let Some(text) = island_text(c, open) else {
        return Ok(());
    };
    let cut = &text[..text.len().saturating_sub(2)];
    if cut.is_empty() {
        return Err(refused_at(
            c.tokens(),
            platform_tok,
            JStr::from("Persistence platform 'AwsGlue' must have a non-empty body"),
        ));
    }
    let island = tokenize(cut);
    let (open_line, open_column) = (c.tokens().start_line(open) as i32, c.tokens().start_column(open) as i32);
    let refused = |message: JStr, line: i32, column: i32| {
        let shift = if line == 1 { open_column + 1 } else { 0 };
        JavaError::parse(message, line + open_line - 1, column + shift).as_refusal()
    };
    let unexpected = |i: usize| {
        refused(
            jcat!("Unexpected token '", island.text(i), "'"),
            island.start_line(i) as i32,
            island.start_column(i) as i32,
        )
    };
    let end = (island.line_of(cut.len()) as i32, island.column_of(cut.len()) as i32);
    let n = island.count();
    let mut i = 0;
    while i < n {
        if !eq(island.text(i), "dataProcessingUnits") {
            return Err(unexpected(i));
        }
        for (offset, wanted) in [(1, T::COLON), (2, T::INTEGER)] {
            if i + offset >= n {
                return Err(refused(JStr::from("Unexpected token '<EOF>'"), end.0, end.1));
            }
            if island.ty(i + offset) != wanted {
                return Err(unexpected(i + offset));
            }
        }
        if i + 3 >= n {
            return Err(refused(JStr::from("Unexpected token"), end.0, end.1 - 1));
        }
        if island.ty(i + 3) != T::SEMI_COLON {
            return Err(unexpected(i + 3));
        }
        i += 4;
    }
    Ok(())
}

/// `parseCtxParams`: `[ name = value, ... ]` -- a connection island, a connection pointer (an identifier that is not a
/// quoted string), or a primitive read by the expression parser to the next top-level `,` or the list's own `]`.
fn parse_ctx_params<'s>(c: &mut impl Cursor<'s>, out: &mut Vec<Box<PCtxParam>>) -> R<()> {
    c.expect(T::BRACKET_OPEN)?;
    while !c.at_end() && c.peek() != T::BRACKET_CLOSE {
        let s = c.pos();
        let name = c.parse_identifier()?;
        c.expect(T::EQUAL)?;
        let value = if is_island(c.peek()) {
            PCtxParamValue::PCtxParamValueConnectionVal(Box::new(PCtxParamValueConnectionVal {
                connection: parse_connection_island(c)?,
            }))
        } else if is_identifier_token(c.peek()) && !c.text().is_empty() && !starts_with(c.text(), "'") {
            let vs = c.pos();
            let head = unquote_path_iae(&qn_admitting_booleans(c)?)?;
            PCtxParamValue::PCtxParamValueConnectionPtr(Box::new(PCtxParamValueConnectionPtr {
                path: JStr(head),
                source_information: c.span(vs, c.pos() - 1),
            }))
        } else {
            let vs = c.pos();
            let mut d = 0i64;
            while !c.at_end() {
                let tk = c.peek();
                match tk {
                    T::PAREN_OPEN | T::BRACE_OPEN | T::BRACKET_OPEN => d += 1,
                    T::PAREN_CLOSE | T::BRACE_CLOSE | T::BRACKET_CLOSE => d -= 1,
                    _ => {}
                }
                if (tk == T::COMMA && d <= 0) || (tk == T::BRACKET_CLOSE && d < 0) {
                    break;
                }
                c.advance()?;
            }
            PCtxParamValue::PCtxParamValuePrimitive(Box::new(PCtxParamValuePrimitive {
                spec: parse_tokens(&c.tokens().slice(vs, c.pos()), c.dialect())?,
            }))
        };
        out.push(Box::new(PCtxParam {
            name: JStr(name),
            value,
            source_information: c.span(s, c.pos() - 1),
        }));
        c.list_comma()?;
    }
    c.expect(T::BRACKET_CLOSE)
}

/// `parseConnectionIsland`: a kindless `#{ Flavor { ... } }#`, re-lexed through the Connection grammar with the
/// walker offsets.
fn parse_connection_island<'s>(c: &mut impl Cursor<'s>) -> R<PConnectionValue> {
    let emb_start = scan_island(c)?;
    let emb = c.reconstruct_text(emb_start, c.pos());
    let level = c.dialect();
    let value = parse_island_value(
        &emb,
        start_line(c.tokens(), emb_start)?,
        start_column(c.tokens(), emb_start)?,
        level,
    );
    let value = match value {
        // legend-engine's walk reads the island: the reading for a later syntax error passes it by
        Err(_) if level.reads_for_syntax() => PConnectionValue::PConnectionPointer(Box::new(PConnectionPointer {
            connection: JStr::default(),
            source_information: None,
        })),
        answer => answer?,
    };
    c.expect(T::ISLAND_END)?;
    Ok(value)
}
