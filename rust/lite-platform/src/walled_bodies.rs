// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `WalledBodies`: Pure bodies the platform carries verbatim (the prelude module's derived properties and constraints,
//! the engine's SQL post-processing entry points) and refuses to compile, each with the reason it is the engine's
//! implementation of a concern the platform serves itself or does not serve. One list, shrink-only.
//!
//! Ported from core's `com.legend.platform.WalledBodies` (2026-11-12, docs/rust-port/PHASE3.md slice 1b): Java's
//! `REASONS` is a `LinkedHashMap`, so its order is insertion order, kept here ([`LinkedMap`]).

use std::sync::LazyLock;

use lite_model::JStr;
use lite_model::linked::LinkedMap;

/// Why a body is walled.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// The engine's machinery for a concern the platform serves itself.
    ENGINE_MACHINERY,
    /// A capability the platform does not model (reflection).
    CANNOT_IMPLEMENT,
}

impl Kind {
    /// Java's `values()`.
    pub const VALUES: [Kind; 2] = [Kind::ENGINE_MACHINERY, Kind::CANNOT_IMPLEMENT];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::ENGINE_MACHINERY => "ENGINE_MACHINERY",
            Kind::CANNOT_IMPLEMENT => "CANNOT_IMPLEMENT",
        }
    }
}

/// One wall: its kind and its reason.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Wall {
    pub kind: Kind,
    pub why: JStr,
}

impl Wall {
    /// `new Wall(kind, why)`.
    pub fn new(kind: Kind, why: &str) -> Wall {
        Wall {
            kind,
            why: JStr::from(why),
        }
    }
}

const PRINTER: &str = concat!(
    "the engine's SQL printer — the platform's compiler is the implementation",
    " (the post-processor design session decides which passes the platform implements)"
);
const SCHEMA: &str = "the engine's plan-time schema inference for TDS operations — the platform's typer infers schemas";

static REASONS: LazyLock<LinkedMap<JStr, Wall>> = LazyLock::new(build);

fn build() -> LinkedMap<JStr, Wall> {
    let mut reasons = LinkedMap::new();
    let mut put = |fqn: String, kind: Kind, why: &str| {
        reasons.put(JStr::from(fqn), Wall::new(kind, why));
    };
    use Kind::{CANNOT_IMPLEMENT, ENGINE_MACHINERY};
    // the SQL post-processing machinery
    for fqn in [
        "meta::relational::functions::sqlQueryToString::sqlQueryToString",
        "meta::relational::runtime::PostProcessor$prop$planPostProcessorId",
        "meta::relational::runtime::PostProcessor$prop$executionPostProcessorId",
        "meta::relational::runtime::PostProcessors$prop$_sqlQueryPostProcessorId",
        "meta::relational::runtime::PostProcessors$prop$sqlQueryPostProcessorId",
    ] {
        put(fqn.to_owned(), ENGINE_MACHINERY, PRINTER);
    }
    // the engine's plan-time context flattening
    put(
        "meta::pure::executionPlan::MultiExecutionContext$prop$allContexts".to_owned(),
        ENGINE_MACHINERY,
        concat!(
            "the engine's plan-time context flattening — the platform reads",
            " ExecutionOptionContext's feature flags directly (ContextReading.contextFeatures)"
        ),
    );
    // the printer's bodies on the prelude's vocabulary classes
    for p in [
        "dataTypeToSqlText",
        "dynaFuncDispatch",
        "joinProcessor",
        "lateralJoinProcessor",
        "literalProcessor",
        "selectSQLQueryProcessor",
        "withinGroupProcessor",
    ] {
        put(
            format!("meta::relational::functions::sqlQueryToString::DbConfig$prop${p}"),
            ENGINE_MACHINERY,
            PRINTER,
        );
    }
    // null-ordering rendering: the printer's, per dialect
    put(
        "meta::relational::functions::sqlQueryToString::NullOrderingSupport$prop$processSortItem".to_owned(),
        ENGINE_MACHINERY,
        PRINTER,
    );
    put(
        "meta::relational::functions::sqlQueryToString::DynaFunctionToSql$prop$toSql".to_owned(),
        ENGINE_MACHINERY,
        PRINTER,
    );
    // SchemaState: plan-time schema inference
    for p in [
        "columnValueDifference",
        "extend",
        "groupBy",
        "join",
        "olap",
        "rename",
        "restrict",
    ] {
        put(
            format!("meta::pure::tds::schema::SchemaState$prop${p}"),
            ENGINE_MACHINERY,
            SCHEMA,
        );
    }
    put(
        "meta::pure::extension::Extension$prop$fetchSerializerExtension".to_owned(),
        ENGINE_MACHINERY,
        "the engine's serializer-extension registry lookup — the platform has its own extensions",
    );
    // the descriptors' constraints call checkSuperType, whose reflection helper is in a refused stdlib-extension file
    for fqn in [
        "meta::external::format::shared::ExternalFormatFromPureDescriptor$constraint$configurationType",
        "meta::external::format::shared::ExternalFormatToPureDescriptor$constraint$configurationType",
    ] {
        put(
            fqn.to_owned(),
            CANNOT_IMPLEMENT,
            "external-format binding validation — its reflection helper is in the refused stdlib-extension files",
        );
    }
    // instantiation from a Class value is reflection; the platform's new is the ^X(...) form
    put(
        "meta::pure::functions::lang::tests::new::testNewGenericFunc".to_owned(),
        CANNOT_IMPLEMENT,
        "REFLECTION: instantiation from a Class value (new(class, id)) — not modeled",
    );
    reasons
}

/// `reasons()`: every walled FQN with its reason, in insertion order (the implementation table's refused rows read
/// it).
pub fn reasons() -> &'static LinkedMap<JStr, Wall> {
    &REASONS
}

/// `reason(fqn)`: the wall reason for a body FQN (a lifted derived property or constraint, or a function), if walled.
pub fn reason(fqn: &str) -> Option<&'static JStr> {
    REASONS.get(&JStr::from(fqn)).map(|w| &w.why)
}

/// `count()`.
pub fn count() -> usize {
    REASONS.len()
}
