// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelOpTranslator` (RelOpTranslator.java at ad86fa895, PORT LINE FOR LINE): every legacy relational expression
//! (property expressions, filters, groupBy keys, view columns, join conditions, association predicates) as Pure. The
//! arm order, the guards' fall-through to the generic arm and the left-to-right evaluation order decide which error
//! surfaces. EF_CENSUS §1.5's families: ROT-1 dispatch (`translate` with `translateTail`, `translateArgs`, `toOne*`,
//! :127-627), ROT-2 `PipelineView`, `joinNavigation`, `rebaseToTable` (:57-91, :632-691), ROT-3 helpers
//! (`collectTablesIn`, `columnRead`, `literalToValueSpec`, `comparisonFn`, :93-115, :693-717), ROT-4 residual-name
//! resolution (:726-773).
//!
//! RE-EXPRESSED (census §4, ruling 31):
//! - `translate` and `translateTail` are ONE match in the same arm order (the split existed for a method-length guard;
//!   `translateTail` is reached only from `translate`'s default arm) -- [`translate`] alone, no `translate_tail`.
//! - The dispatch's dynafunction arms are `crate::dyna_fn::arm`'s [`Arm`](crate::dyna_fn::Arm): `translate` matches
//!   `arm(d)` EXHAUSTIVELY over `Arm` (no `DynaFn::X` guard of its own), which is `DynaFnRegistryTest`'s
//!   source-text invariant made structural.
//! - `PipelineView` is `Option<&Pipeline>` (`crate::pipeline::PipelineView`); `NONE` is `None`.
//! - `operatorCall`'s `shaped == args` (:771) is the boolean it tests: `args.len() >= 2 && isVariadicRun(fqns)`.
//! - The raw internal errors a user's text could reach (ASK 11) are refusals in Java since ad86fa895 (ruling 29;
//!   :720's TRANSLATED arm a `NotImplementedException` now); `literalToValueSpec`'s text names the literal's Java class
//!   (`JValue::java_class`).

use std::sync::Arc;

use lite_builtin::dyna_fn::DynaFn;
use lite_front::records as p;
use lite_model::linked::LinkedMap;
use lite_model::{
    ComparisonOp, JStr, JValue, RelationalOperation, RelationalOperationColumnRef, RelationalOperationFunctionCall,
    RelationalOperationJoinNavigation,
};

use crate::error::Thrown;
use crate::jdk::LinkedSet;
use crate::pipeline::Pipeline;
use crate::unported;

/// The table scope a translation reads: table name -> the row expression its columns read off (Java's
/// `Map<String, ValueSpecification> tableScope`, a `LinkedHashMap` at every site; its `keySet()` is printed in a
/// wall text).
pub type TableScope = LinkedMap<JStr, p::ValueSpecification>;

/// `RelOpTranslator.DYNA_HASH_TYPES` (:47): the hash dynafunctions' algorithm names.
pub fn dyna_hash_type(d: DynaFn) -> Option<&'static str> {
    match d {
        DynaFn::MD5 => Some("MD5"),
        DynaFn::SHA1 => Some("SHA1"),
        DynaFn::SHA256 => Some("SHA256"),
        _ => None,
    }
}

/// `RelOpTranslator.dyna` (:52): the registry's entry for the call's name.
pub fn dyna(call: &RelationalOperationFunctionCall) -> Option<DynaFn> {
    // UNPORTED: RelOpTranslator.dyna
    unported!("RelOpTranslator.dyna")
}

/// `RelOpTranslator.collectTablesIn` (:93): the tables an expression reads, into `sink`; loud for a join navigation
/// inside it.
pub fn collect_tables_in(op: &RelationalOperation, sink: &mut LinkedSet<JStr>) -> Result<(), Thrown> {
    Err(Thrown::Unported("RelOpTranslator.collectTablesIn"))
}

/// `RelOpTranslator.columnRead` (:101): `<row>.column` for the table's row in scope (the default table's when the
/// table has none); loud for an ambiguous table or no row.
pub fn column_read(
    table: &JStr,
    column: &JStr,
    table_scope: &TableScope,
    default_table: &JStr,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.columnRead"))
}

/// `RelOpTranslator.translateArgs` (:127): a call's arguments translated, left to right.
pub fn translate_args(
    call: &RelationalOperationFunctionCall,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<Vec<p::ValueSpecification>, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.translateArgs"))
}

/// `RelOpTranslator.toOne` (:150): `trustOne(v)`.
pub fn to_one(v: &p::ValueSpecification) -> p::ValueSpecification {
    // UNPORTED: RelOpTranslator.toOne
    unported!("RelOpTranslator.toOne")
}

/// `RelOpTranslator.toOneAll` (:154).
pub fn to_one_all(vs: &[p::ValueSpecification]) -> Vec<p::ValueSpecification> {
    // UNPORTED: RelOpTranslator.toOneAll
    unported!("RelOpTranslator.toOneAll")
}

/// `RelOpTranslator.strCast` (:164).
pub fn str_cast(v: &p::ValueSpecification) -> p::ValueSpecification {
    // UNPORTED: RelOpTranslator.strCast
    unported!("RelOpTranslator.strCast")
}

/// `RelOpTranslator.pureTypeFor` (:173): a `convertVarchar`/`cast` SQL type's Pure type name; loud for another.
pub fn pure_type_for(sql_type: &JStr) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.pureTypeFor"))
}

/// `RelOpTranslator.ambiguousTableRef` (:208): the refusal of a bare column ref to a table two sub-row slots reach.
pub fn ambiguous_table_ref(table: &JStr, column: &JStr) -> Thrown {
    // UNPORTED: RelOpTranslator.ambiguousTableRef
    unported!("RelOpTranslator.ambiguousTableRef")
}

/// `RelOpTranslator.translate` (:217, with `translateTail` :398 as its tail; module note): one relational expression
/// as Pure.
pub fn translate(
    op: &RelationalOperation,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.translate"))
}

/// `RelOpTranslator.joinNavigation` (:632): `$row.<slot>.<col>` for a hoisted join navigation (an inner scope over
/// the slot's target table otherwise).
pub fn join_navigation(
    jn: &RelationalOperationJoinNavigation,
    table_scope: &TableScope,
    target_var_or_null: Option<&p::ValueSpecification>,
    row_bind_or_null: Option<&Arc<p::Variable>>,
    pipeline: Option<&Pipeline<'_>>,
) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.joinNavigation"))
}

/// `RelOpTranslator.rebaseToTable` (:680): the column refs `moves` accepts re-pointed at `table`.
pub fn rebase_to_table(
    t: &RelationalOperation,
    table: &JStr,
    moves: &dyn Fn(&RelationalOperationColumnRef) -> bool,
) -> RelationalOperation {
    // UNPORTED: RelOpTranslator.rebaseToTable
    unported!("RelOpTranslator.rebaseToTable")
}

/// `RelOpTranslator.literalToValueSpec` (:693): a literal as a Pure constant; loud for another kind ("Unsupported
/// literal type: <Java class>").
pub fn literal_to_value_spec(value: &JValue) -> Result<p::ValueSpecification, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.literalToValueSpec"))
}

/// `RelOpTranslator.comparisonFn` (:703): a comparison's function (`equal`, or the Any-typed Lite ordering shims).
pub fn comparison_fn(op: ComparisonOp) -> &'static str {
    // UNPORTED: RelOpTranslator.comparisonFn
    unported!("RelOpTranslator.comparisonFn")
}

/// `RelOpTranslator.dynaFnName` (:726): a non-armed dynafunction's spelling (PURE: the call's name; SHIM: its Lite
/// FQN); a TRANSLATED one in a shape no arm rewrites, or an UNSUPPORTED one, refused by name.
pub fn dyna_fn_name(call: &RelationalOperationFunctionCall) -> Result<JStr, Thrown> {
    Err(Thrown::Unported("RelOpTranslator.dynaFnName"))
}

/// `RelOpTranslator.variadicRun` (:752): two or more arguments to a variadic-run name as one collection.
pub fn variadic_run(f: &JStr, fqns: &[JStr], args: Vec<p::ValueSpecification>) -> Vec<p::ValueSpecification> {
    // UNPORTED: RelOpTranslator.variadicRun
    unported!("RelOpTranslator.variadicRun")
}

/// `RelOpTranslator.dynaFnCandidates` (:761): a PURE dynafunction's catalog FQNs, else none.
pub fn dyna_fn_candidates(call: &RelationalOperationFunctionCall) -> Vec<JStr> {
    // UNPORTED: RelOpTranslator.dynaFnCandidates
    unported!("RelOpTranslator.dynaFnCandidates")
}

/// `RelOpTranslator.operatorCall` (:770): `Calls.among`, or `Calls.infixRunAmong` for a variadic run (module note on
/// `shaped == args`).
pub fn operator_call(f: &JStr, candidates: &[JStr], args: Vec<p::ValueSpecification>) -> p::AppliedFunction {
    // UNPORTED: RelOpTranslator.operatorCall
    unported!("RelOpTranslator.operatorCall")
}
