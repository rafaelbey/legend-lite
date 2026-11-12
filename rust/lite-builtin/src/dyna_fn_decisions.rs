// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `DynaFnDecisions` and `DynaFn`'s methods: how this platform resolves each engine dynafunction (the generated
//! registry, [`crate::dyna_fn`]), written by hand -- PURE (the engine's operator is Pure's function: its catalog
//! declarations), SHIM (an engine-only operator: its `Pure.Lite` identity), TRANSLATED (the mapping translator rewrites
//! the call), UNSUPPORTED (every name not listed). A decision names a generated member, so a name an upgrade drops does
//! not compile here.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::dyna_fn::{DynaFn, Inference, ROWS};
use crate::natives;
use crate::pure::lite;

/// `DynaFn.Resolution`.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resolution {
    PURE,
    SHIM,
    TRANSLATED,
    UNSUPPORTED,
}

impl Resolution {
    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Resolution::PURE => "PURE",
            Resolution::SHIM => "SHIM",
            Resolution::TRANSLATED => "TRANSLATED",
            Resolution::UNSUPPORTED => "UNSUPPORTED",
        }
    }
}

/// A decision: its resolution, and a SHIM's Lite FQN.
#[derive(Clone, Copy, Debug)]
enum Decision {
    Pure,
    Translated,
    Shim(&'static str),
}

/// By member (the engine's operator name), alphabetical. Unlisted: UNSUPPORTED.
const BY_MEMBER: &[(DynaFn, Decision)] = &[
    (DynaFn::ABS, Decision::Pure),
    (DynaFn::ACOS, Decision::Pure),
    (DynaFn::ADD, Decision::Translated),
    (DynaFn::ADJUST, Decision::Translated),
    (DynaFn::AND, Decision::Pure),
    (DynaFn::ASCII, Decision::Pure),
    (DynaFn::ASIN, Decision::Pure),
    (DynaFn::ATAN, Decision::Pure),
    (DynaFn::ATAN2, Decision::Pure),
    (DynaFn::AVERAGE, Decision::Pure),
    (DynaFn::BETWEEN, Decision::Pure),
    (DynaFn::BIT_AND, Decision::Pure),
    (DynaFn::BIT_NOT, Decision::Pure),
    (DynaFn::BIT_OR, Decision::Pure),
    (DynaFn::BIT_SHIFT_LEFT, Decision::Pure),
    (DynaFn::BIT_SHIFT_RIGHT, Decision::Pure),
    (DynaFn::BIT_XOR, Decision::Pure),
    (DynaFn::CASE, Decision::Translated),
    (DynaFn::CAST, Decision::Pure),
    (DynaFn::CBRT, Decision::Pure),
    (DynaFn::CEILING, Decision::Pure),
    (DynaFn::CHAR, Decision::Pure),
    (DynaFn::COALESCE, Decision::Pure),
    (DynaFn::CONCAT, Decision::Translated),
    (DynaFn::CONTAINS, Decision::Pure),
    (DynaFn::CONVERT_DATE, Decision::Translated),
    (DynaFn::CONVERT_DATE_TIME, Decision::Translated),
    (DynaFn::CONVERT_TIME_ZONE, Decision::Translated),
    (DynaFn::CONVERT_VARCHAR128, Decision::Translated),
    (DynaFn::CORR, Decision::Pure),
    (DynaFn::COS, Decision::Pure),
    (DynaFn::COSH, Decision::Pure),
    (DynaFn::COT, Decision::Pure),
    (DynaFn::COUNT, Decision::Pure),
    (DynaFn::COVAR_POPULATION, Decision::Pure),
    (DynaFn::COVAR_SAMPLE, Decision::Pure),
    (DynaFn::CUMULATIVE_DISTRIBUTION, Decision::Pure),
    (DynaFn::CURRENT_USER_ID, Decision::Pure),
    (DynaFn::DATE, Decision::Pure),
    (DynaFn::DATE_DIFF, Decision::Pure),
    (DynaFn::DATE_PART, Decision::Pure),
    (DynaFn::DAY_OF_MONTH, Decision::Pure),
    (DynaFn::DAY_OF_WEEK, Decision::Translated),
    (DynaFn::DAY_OF_WEEK_NUMBER, Decision::Translated),
    (DynaFn::DAY_OF_YEAR, Decision::Pure),
    (DynaFn::DECODE_BASE64, Decision::Pure),
    (DynaFn::DENSE_RANK, Decision::Pure),
    (DynaFn::DISTINCT, Decision::Pure),
    (DynaFn::DIVIDE, Decision::Pure),
    (DynaFn::DIVIDE_ROUND, Decision::Shim(lite::DIVIDE_ROUND)),
    (DynaFn::ENCODE_BASE64, Decision::Pure),
    (DynaFn::ENDS_WITH, Decision::Pure),
    (DynaFn::EQUAL, Decision::Pure),
    (DynaFn::EXISTS, Decision::Pure),
    (DynaFn::EXP, Decision::Pure),
    (DynaFn::EXTRACT_FROM_SEMI_STRUCTURED, Decision::Translated),
    (DynaFn::FIRST, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_MONTH, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_QUARTER, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_THIS_MONTH, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_THIS_QUARTER, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_THIS_YEAR, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_WEEK, Decision::Pure),
    (DynaFn::FIRST_DAY_OF_YEAR, Decision::Pure),
    (DynaFn::FIRST_HOUR_OF_DAY, Decision::Pure),
    (DynaFn::FIRST_MILLISECOND_OF_SECOND, Decision::Pure),
    (DynaFn::FIRST_MINUTE_OF_HOUR, Decision::Pure),
    (DynaFn::FIRST_SECOND_OF_MINUTE, Decision::Pure),
    (DynaFn::FLOOR, Decision::Pure),
    (DynaFn::FORMAT_DATE, Decision::Pure),
    (DynaFn::GENERATE_GUID, Decision::Pure),
    (DynaFn::GREATER_THAN, Decision::Shim(lite::GREATER_THAN_ANY)),
    (DynaFn::GREATER_THAN_EQUAL, Decision::Shim(lite::GREATER_THAN_EQUAL_ANY)),
    (DynaFn::GREATEST, Decision::Pure),
    (DynaFn::GROUP, Decision::Translated),
    (DynaFn::HASH_CODE, Decision::Pure),
    (DynaFn::HOUR, Decision::Pure),
    (DynaFn::IF, Decision::Translated),
    (DynaFn::IN, Decision::Pure),
    (DynaFn::INDEX_OF, Decision::Translated),
    (DynaFn::IS_ALPHA_NUMERIC, Decision::Pure),
    (DynaFn::IS_DISTINCT, Decision::Shim(lite::IS_DISTINCT_FROM)),
    (DynaFn::IS_EMPTY, Decision::Pure),
    (DynaFn::IS_NOT_EMPTY, Decision::Pure),
    (DynaFn::IS_NOT_NULL, Decision::Translated),
    (DynaFn::IS_NULL, Decision::Translated),
    (DynaFn::IS_NUMERIC, Decision::Shim(lite::IS_NUMERIC)),
    (DynaFn::JARO_WINKLER_SIMILARITY, Decision::Pure),
    (DynaFn::JOIN_STRINGS, Decision::Pure),
    (DynaFn::KEYS, Decision::Pure),
    (DynaFn::LAG, Decision::Pure),
    (DynaFn::LAST, Decision::Pure),
    (DynaFn::LEAD, Decision::Pure),
    (DynaFn::LEAST, Decision::Pure),
    (DynaFn::LEFT, Decision::Pure),
    (DynaFn::LENGTH, Decision::Pure),
    (DynaFn::LESS_THAN, Decision::Shim(lite::LESS_THAN_ANY)),
    (DynaFn::LESS_THAN_EQUAL, Decision::Shim(lite::LESS_THAN_EQUAL_ANY)),
    (DynaFn::LEVENSHTEIN_DISTANCE, Decision::Pure),
    (DynaFn::LOG, Decision::Pure),
    (DynaFn::LOG10, Decision::Pure),
    (DynaFn::LPAD, Decision::Pure),
    (DynaFn::LTRIM, Decision::Pure),
    (DynaFn::MATCHES, Decision::Pure),
    (DynaFn::MAX, Decision::Pure),
    (DynaFn::MAX_BY, Decision::Pure),
    (DynaFn::MD5, Decision::Translated),
    (DynaFn::MEDIAN, Decision::Pure),
    (DynaFn::MIN, Decision::Pure),
    (DynaFn::MIN_BY, Decision::Pure),
    (DynaFn::MINUS, Decision::Pure),
    (DynaFn::MINUTE, Decision::Pure),
    (DynaFn::MOD, Decision::Pure),
    (DynaFn::MODE, Decision::Pure),
    (DynaFn::MONTH, Decision::Pure),
    (DynaFn::MONTH_NUMBER, Decision::Pure),
    (DynaFn::MOST_RECENT_DAY_OF_WEEK, Decision::Pure),
    (DynaFn::NOT, Decision::Pure),
    (DynaFn::NOT_EQUAL_ANSI, Decision::Shim(lite::NOT_EQUAL_ANSI)),
    (DynaFn::NOW, Decision::Pure),
    (DynaFn::NTH, Decision::Pure),
    (DynaFn::NTILE, Decision::Pure),
    (DynaFn::OBJECT_REFERENCE_IN, Decision::Pure),
    (DynaFn::OR, Decision::Pure),
    (DynaFn::PARSE_BOOLEAN, Decision::Pure),
    (DynaFn::PARSE_DATE, Decision::Pure),
    (DynaFn::PARSE_DECIMAL, Decision::Pure),
    (DynaFn::PARSE_FLOAT, Decision::Pure),
    (DynaFn::PARSE_INTEGER, Decision::Pure),
    (DynaFn::PERCENT_RANK, Decision::Pure),
    (DynaFn::PERCENTILE, Decision::Pure),
    (DynaFn::PLUS, Decision::Pure),
    (DynaFn::POSITION, Decision::Translated),
    (DynaFn::POW, Decision::Pure),
    (DynaFn::PREVIOUS_DAY_OF_WEEK, Decision::Pure),
    (DynaFn::QUARTER, Decision::Pure),
    (DynaFn::QUARTER_NUMBER, Decision::Pure),
    (DynaFn::RANGE, Decision::Pure),
    (DynaFn::RANK, Decision::Pure),
    (DynaFn::REGEXP_COUNT, Decision::Pure),
    (DynaFn::REGEXP_EXTRACT, Decision::Pure),
    (DynaFn::REGEXP_INDEX_OF, Decision::Pure),
    (DynaFn::REGEXP_LIKE, Decision::Pure),
    (DynaFn::REGEXP_REPLACE, Decision::Pure),
    (DynaFn::REM, Decision::Pure),
    (DynaFn::REPEAT_STRING, Decision::Pure),
    (DynaFn::REPLACE, Decision::Pure),
    (DynaFn::REVERSE_STRING, Decision::Pure),
    (DynaFn::RIGHT, Decision::Pure),
    (DynaFn::ROUND, Decision::Pure),
    (DynaFn::ROW_NUMBER, Decision::Pure),
    (DynaFn::RPAD, Decision::Pure),
    (DynaFn::RTRIM, Decision::Pure),
    (DynaFn::SECOND, Decision::Pure),
    (DynaFn::SHA1, Decision::Translated),
    (DynaFn::SHA256, Decision::Translated),
    (DynaFn::SIGN, Decision::Pure),
    (DynaFn::SIN, Decision::Pure),
    (DynaFn::SINH, Decision::Pure),
    (DynaFn::SIZE, Decision::Pure),
    (DynaFn::SPLIT_PART, Decision::Translated),
    (DynaFn::SQL_FALSE, Decision::Pure),
    (DynaFn::SQL_NULL, Decision::Pure),
    (DynaFn::SQL_TRUE, Decision::Pure),
    (DynaFn::SQRT, Decision::Pure),
    (DynaFn::STARTS_WITH, Decision::Pure),
    (DynaFn::STD_DEV_POPULATION, Decision::Pure),
    (DynaFn::STD_DEV_SAMPLE, Decision::Pure),
    (DynaFn::SUB, Decision::Translated),
    (DynaFn::SUBSTRING, Decision::Translated),
    (DynaFn::SUM, Decision::Pure),
    (DynaFn::TAN, Decision::Pure),
    (DynaFn::TANH, Decision::Pure),
    (DynaFn::TIME_BUCKET, Decision::Pure),
    (DynaFn::TIMES, Decision::Pure),
    (DynaFn::TO_DECIMAL, Decision::Pure),
    (DynaFn::TO_FLOAT, Decision::Pure),
    (DynaFn::TO_LOWER, Decision::Pure),
    (DynaFn::TO_ONE, Decision::Pure),
    (DynaFn::TO_STRING, Decision::Pure),
    (DynaFn::TO_TIMESTAMP, Decision::Translated),
    (DynaFn::TO_UPPER, Decision::Pure),
    (DynaFn::TO_VARIANT, Decision::Pure),
    (DynaFn::TODAY, Decision::Pure),
    (DynaFn::TRIM, Decision::Pure),
    (DynaFn::VALUES, Decision::Pure),
    (DynaFn::VARIANCE, Decision::Pure),
    (DynaFn::VARIANCE_POPULATION, Decision::Pure),
    (DynaFn::VARIANCE_SAMPLE, Decision::Pure),
    (DynaFn::WEEK_OF_YEAR, Decision::Pure),
    (DynaFn::YEAR, Decision::Pure),
];

/// The three engine operators no handler names: the platform's own relational constants, each spelled by its catalog
/// declaration.
fn residue(d: DynaFn) -> Option<Vec<String>> {
    let n = match d {
        DynaFn::SQL_NULL => natives::SQL_NULL,
        DynaFn::SQL_TRUE => natives::SQL_TRUE,
        DynaFn::SQL_FALSE => natives::SQL_FALSE,
        _ => return None,
    };
    Some(vec![n.def().qualified_name.lossy()])
}

fn decision(d: DynaFn) -> Option<Decision> {
    BY_MEMBER.iter().find(|(m, _)| *m == d).map(|(_, x)| *x)
}

/// Each member's declarations, computed once, at first use (the engine surface joined when `EngineHandlers` loads).
static DECLARATIONS: LazyLock<HashMap<DynaFn, Vec<String>>> = LazyLock::new(|| {
    let mut of = HashMap::new();
    for (d, _, _) in ROWS {
        let fqns = match decision(d) {
            Some(Decision::Pure) => {
                let at = crate::engine_handlers::handlers().fqns_of(d.dyna_name());
                if at.is_empty() {
                    residue(d).unwrap_or_default()
                } else {
                    at.to_vec()
                }
            }
            Some(Decision::Shim(fqn)) => vec![fqn.to_owned()],
            Some(Decision::Translated) | None => Vec::new(),
        };
        of.insert(d, fqns);
    }
    of
});

impl DynaFn {
    /// Every member, in the enum's order (Java's `values()`).
    pub fn values() -> impl Iterator<Item = DynaFn> {
        ROWS.iter().map(|(d, _, _)| *d)
    }

    fn row(self) -> &'static (DynaFn, &'static str, Inference) {
        ROWS.iter()
            .find(|(d, _, _)| *d == self)
            .expect("every member has its row")
    }

    /// The engine's spelling of the operator.
    pub fn dyna_name(self) -> &'static str {
        self.row().1
    }

    /// Whether the engine's type-inference map has a rule for the name.
    pub fn inference(self) -> Inference {
        self.row().2
    }

    /// The platform's decision for the name (UNSUPPORTED unless one says otherwise).
    pub fn resolution(self) -> Resolution {
        match decision(self) {
            None => Resolution::UNSUPPORTED,
            Some(Decision::Pure) => Resolution::PURE,
            Some(Decision::Translated) => Resolution::TRANSLATED,
            Some(Decision::Shim(_)) => Resolution::SHIM,
        }
    }

    /// The declarations the name resolves to: a PURE name's catalog FQNs (the engine surface's, or the declared
    /// residue), a SHIM's one `Pure.Lite` FQN; empty for TRANSLATED and UNSUPPORTED.
    pub fn fqns(self) -> &'static [String] {
        DECLARATIONS.get(&self).map_or(&[], |v| v.as_slice())
    }

    /// The `Pure.Lite` identity a SHIM resolves to; another member is refused.
    pub fn lite_fqn(self) -> Result<&'static str, String> {
        let fqns = self.fqns();
        if self.resolution() != Resolution::SHIM || fqns.len() != 1 {
            return Err(format!("{} is not a SHIM", self.dyna_name()));
        }
        Ok(&fqns[0])
    }

    /// `DynaFn.of(name)`: the registry entry for an engine operator name.
    pub fn of(dyna_name: &str) -> Option<DynaFn> {
        ROWS.iter().find(|(_, n, _)| *n == dyna_name).map(|(d, _, _)| *d)
    }

    /// Every member of one resolution kind, in the enum's order.
    pub fn with_resolution(r: Resolution) -> Vec<DynaFn> {
        DynaFn::values().filter(|d| d.resolution() == r).collect()
    }
}

impl Inference {
    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Inference::MAPPED => "MAPPED",
            Inference::NONE => "NONE",
        }
    }
}

/// The Lite constants a SHIM names, for the record (`Pure.ENGINE_VOCAB_SHIMS`' landing set).
pub const SHIM_TARGETS: &[&str] = &[
    lite::DIVIDE_ROUND,
    lite::GREATER_THAN_ANY,
    lite::GREATER_THAN_EQUAL_ANY,
    lite::IS_DISTINCT_FROM,
    lite::IS_NUMERIC,
    lite::LESS_THAN_ANY,
    lite::LESS_THAN_EQUAL_ANY,
    lite::NOT_EQUAL_ANSI,
];
