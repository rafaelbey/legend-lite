// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `MappingNormalizer` (MappingNormalizer.java at ad86fa895, 2,860 lines): E.1, the legacy mapping DSL desugared into
//! synthesized realizing functions and the canonical binding table. One file per family group of EF_CENSUS §1.1 (H1
//! has the evidence), so the fill can be split; every function is re-exported here, so a caller names
//! `mapping_normalizer::f` wherever `f` lives.
//!
//! | file | families | Java lines |
//! |---|---|---|
//! | [`entry`] | 1 entry and per-mapping wall (`normalize`, `withElement`), 2 legacy mapping -> binding table | 152-462 |
//! | [`clean_sheet`] | 3 the clean-sheet door and its inline lifts | 482-693 |
//! | [`xstore`] | 4 the XStore direction check (`canonicalizeEqualOperands`, `rootedAt`, `nameRefOrNull`), 7 XStore / ModelJoin predicates | 706-767, 946-1148 |
//! | [`dispatch`] | 5 class-synthesis dispatch | 771-804 |
//! | [`relation_function`] | 6 Relation (`~func`) sets | 817-934 |
//! | [`m2m`] | 8 M2M (Pure) sets | 1155-1257 |
//! | [`relational`] | 10 relational dispatch and main-table inference, 11 JSON-source sets, 12 view-backed sets | 1327-1646 |
//! | [`table_backed`] | 13 the table-backed pipeline | 1655-1983 |
//! | [`fields`] | 14 PM -> constructor field; embedded / otherwise / inline | 1999-2234 |
//! | [`filter`] | 15 `~filter` | 2240-2323 |
//! | [`main_table`] | 16 closure main-table lookups | 2333-2380 |
//! | [`enumeration`] | 17 enumeration decode | 2386-2498 |
//! | [`alias_scope`] | 18 alias scope, view refs in joins, join target | 2504-2718 |
//! | [`instances`] | 19 instance construction, `trustOne`, booleanize | 2729-2854 |
//!
//! Not ported (EF_CENSUS §3): the public 2- and 3-argument `normalize` (:135, :142; test-only), `withElement`'s
//! `MappingResolutionException` arm (:246-247), family 9's `collectColumnsOfTable` and `collectTargetColumns`
//! (:1266-1321, self-recursive only), `mixedUnionRooted` (:280, :287) and the bare block (:300-353) inside
//! `normalizeMapping`, and the dangling javadoc.

pub mod alias_scope;
pub mod clean_sheet;
pub mod dispatch;
pub mod entry;
pub mod enumeration;
pub mod fields;
pub mod filter;
pub mod instances;
pub mod m2m;
pub mod main_table;
pub mod relation_function;
pub mod relational;
pub mod table_backed;
pub mod xstore;

pub use alias_scope::*;
pub use clean_sheet::*;
pub use dispatch::*;
pub use entry::*;
pub use enumeration::*;
pub use fields::*;
pub use filter::*;
pub use instances::*;
pub use m2m::*;
pub use main_table::*;
pub use relation_function::*;
pub use relational::*;
pub use table_backed::*;
pub use xstore::*;

use lite_front::records as p;
use lite_model::JStr;
use lite_model::linked::LinkedMap;

/// The constructor fields a synthesis binds, property name -> its `KeyExpression`, in binding order (Java's
/// `Map<String, KeyExpression> fields`, a `LinkedHashMap` at every site).
pub type Fields = LinkedMap<JStr, p::KeyExpression>;
