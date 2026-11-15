// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The mapping translator's arm facts (Java's `DynaFnArms`, ported by the owner's ruling 31: `DynaFnRegistryTest` becomes
//! one exhaustive match in Rust; EF_CENSUS ASK 14 (b)).
//!
//! - [`arm`]: ONE exhaustive `match` over lite-builtin's [`DynaFn`] (no `_` arm: a dynafunction the registry gains does
//!   not compile until it is classified here), answering the translator's [`Arm`] for every engine dynafunction
//!   `RelOpTranslator` has a rewriting arm for, `None` for the rest. [`arms`] is Java's `DynaFnArms.ARMS`, computed
//!   from it.
//! - [`Arm::landing`]: Java's `DynaFnArms.LANDINGS`, the `Pure.Lite` identities the FORMAT arms land on (engine
//!   vocabulary with a shape pure lacks, a format argument), one exhaustive `match` over [`Arm`].
//!
//! `DynaFnRegistryTest`'s invariants, as this module's tests: every TRANSLATED name is armed, every armed name is
//! TRANSLATED or PURE (`resolutionsHold`); the landings with the registry's SHIM rows are `Pure.ENGINE_VOCAB_SHIMS`
//! (`engineVocabShimsAreDerivedFromTheRegistry`); the arm set and the landings are Java's, member for member. The
//! third, `armsAreDerivedFromTheTranslatorSource` (Java's arm set IS the `DynaFn` members `RelOpTranslator.java`
//! names), is structural here: `RelOpTranslator::translate` dispatches on [`arm`] and matches [`Arm`] exhaustively,
//! so the translator has an arm for exactly the members [`arm`] arms (its fill must keep that shape: no `DynaFn::X`
//! guard of its own).

use lite_builtin::dyna_fn::DynaFn;
use lite_builtin::pure::lite;

/// A translator arm: one per engine dynafunction `RelOpTranslator` rewrites (a member of Java's `DynaFnArms.ARMS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arm {
    Add,
    Adjust,
    And,
    Case,
    Concat,
    ConvertDate,
    ConvertDateTime,
    ConvertTimeZone,
    ConvertVarchar128,
    DayOfWeek,
    DayOfWeekNumber,
    ExtractFromSemiStructured,
    Group,
    If,
    IndexOf,
    IsNotNull,
    IsNull,
    Md5,
    Or,
    ParseDate,
    Position,
    Sha1,
    Sha256,
    SplitPart,
    Sub,
    Substring,
    ToTimestamp,
}

impl Arm {
    /// The dynafunction this arm rewrites.
    pub fn dyna_fn(self) -> DynaFn {
        match self {
            Arm::Add => DynaFn::ADD,
            Arm::Adjust => DynaFn::ADJUST,
            Arm::And => DynaFn::AND,
            Arm::Case => DynaFn::CASE,
            Arm::Concat => DynaFn::CONCAT,
            Arm::ConvertDate => DynaFn::CONVERT_DATE,
            Arm::ConvertDateTime => DynaFn::CONVERT_DATE_TIME,
            Arm::ConvertTimeZone => DynaFn::CONVERT_TIME_ZONE,
            Arm::ConvertVarchar128 => DynaFn::CONVERT_VARCHAR128,
            Arm::DayOfWeek => DynaFn::DAY_OF_WEEK,
            Arm::DayOfWeekNumber => DynaFn::DAY_OF_WEEK_NUMBER,
            Arm::ExtractFromSemiStructured => DynaFn::EXTRACT_FROM_SEMI_STRUCTURED,
            Arm::Group => DynaFn::GROUP,
            Arm::If => DynaFn::IF,
            Arm::IndexOf => DynaFn::INDEX_OF,
            Arm::IsNotNull => DynaFn::IS_NOT_NULL,
            Arm::IsNull => DynaFn::IS_NULL,
            Arm::Md5 => DynaFn::MD5,
            Arm::Or => DynaFn::OR,
            Arm::ParseDate => DynaFn::PARSE_DATE,
            Arm::Position => DynaFn::POSITION,
            Arm::Sha1 => DynaFn::SHA1,
            Arm::Sha256 => DynaFn::SHA256,
            Arm::SplitPart => DynaFn::SPLIT_PART,
            Arm::Sub => DynaFn::SUB,
            Arm::Substring => DynaFn::SUBSTRING,
            Arm::ToTimestamp => DynaFn::TO_TIMESTAMP,
        }
    }

    /// `DynaFnArms.LANDINGS.get(dynaFn)`: the `Pure.Lite` identity a FORMAT arm lands on.
    pub fn landing(self) -> Option<&'static str> {
        match self {
            Arm::ConvertDate => Some(lite::CONVERT_DATE_FORMAT),
            Arm::ConvertDateTime => Some(lite::CONVERT_DATE_TIME_FORMAT),
            Arm::ConvertTimeZone => Some(lite::CONVERT_TIME_ZONE_FORMAT),
            Arm::ParseDate => Some(lite::PARSE_DATE_FORMAT),
            Arm::ToTimestamp => Some(lite::CONVERT_DATE_TIME_FORMAT),
            Arm::Add
            | Arm::Adjust
            | Arm::And
            | Arm::Case
            | Arm::Concat
            | Arm::ConvertVarchar128
            | Arm::DayOfWeek
            | Arm::DayOfWeekNumber
            | Arm::ExtractFromSemiStructured
            | Arm::Group
            | Arm::If
            | Arm::IndexOf
            | Arm::IsNotNull
            | Arm::IsNull
            | Arm::Md5
            | Arm::Or
            | Arm::Position
            | Arm::Sha1
            | Arm::Sha256
            | Arm::SplitPart
            | Arm::Sub
            | Arm::Substring => None,
        }
    }
}

/// The translator's arm for `d`, if it has one (`DynaFnArms.ARMS.contains(d)`): one exhaustive match.
pub fn arm(d: DynaFn) -> Option<Arm> {
    match d {
        DynaFn::ADD => Some(Arm::Add),
        DynaFn::ADJUST => Some(Arm::Adjust),
        DynaFn::AND => Some(Arm::And),
        DynaFn::CASE => Some(Arm::Case),
        DynaFn::CONCAT => Some(Arm::Concat),
        DynaFn::CONVERT_DATE => Some(Arm::ConvertDate),
        DynaFn::CONVERT_DATE_TIME => Some(Arm::ConvertDateTime),
        DynaFn::CONVERT_TIME_ZONE => Some(Arm::ConvertTimeZone),
        DynaFn::CONVERT_VARCHAR128 => Some(Arm::ConvertVarchar128),
        DynaFn::DAY_OF_WEEK => Some(Arm::DayOfWeek),
        DynaFn::DAY_OF_WEEK_NUMBER => Some(Arm::DayOfWeekNumber),
        DynaFn::EXTRACT_FROM_SEMI_STRUCTURED => Some(Arm::ExtractFromSemiStructured),
        DynaFn::GROUP => Some(Arm::Group),
        DynaFn::IF => Some(Arm::If),
        DynaFn::INDEX_OF => Some(Arm::IndexOf),
        DynaFn::IS_NOT_NULL => Some(Arm::IsNotNull),
        DynaFn::IS_NULL => Some(Arm::IsNull),
        DynaFn::MD5 => Some(Arm::Md5),
        DynaFn::OR => Some(Arm::Or),
        DynaFn::PARSE_DATE => Some(Arm::ParseDate),
        DynaFn::POSITION => Some(Arm::Position),
        DynaFn::SHA1 => Some(Arm::Sha1),
        DynaFn::SHA256 => Some(Arm::Sha256),
        DynaFn::SPLIT_PART => Some(Arm::SplitPart),
        DynaFn::SUB => Some(Arm::Sub),
        DynaFn::SUBSTRING => Some(Arm::Substring),
        DynaFn::TO_TIMESTAMP => Some(Arm::ToTimestamp),
        DynaFn::ABS
        | DynaFn::ACOS
        | DynaFn::ALL_OF
        | DynaFn::ANY_OF
        | DynaFn::ARRAY_APPEND
        | DynaFn::ARRAY_CONCATENATE
        | DynaFn::ARRAY_CONTAINS
        | DynaFn::ARRAY_DISTINCT
        | DynaFn::ARRAY_DROP
        | DynaFn::ARRAY_FIRST
        | DynaFn::ARRAY_FLATTEN
        | DynaFn::ARRAY_INIT
        | DynaFn::ARRAY_LAST
        | DynaFn::ARRAY_MAX
        | DynaFn::ARRAY_MIN
        | DynaFn::ARRAY_POSITION
        | DynaFn::ARRAY_REVERSE
        | DynaFn::ARRAY_SIZE
        | DynaFn::ARRAY_SLICE
        | DynaFn::ARRAY_SORT
        | DynaFn::ARRAY_SUM
        | DynaFn::ARRAY_TAIL
        | DynaFn::ARRAY_TAKE
        | DynaFn::ARRAY_TO_STRING
        | DynaFn::ASCII
        | DynaFn::ASIN
        | DynaFn::ATAN
        | DynaFn::ATAN2
        | DynaFn::AVERAGE
        | DynaFn::AVERAGE_RANK
        | DynaFn::BETWEEN
        | DynaFn::BIT_AND
        | DynaFn::BIT_NOT
        | DynaFn::BIT_OR
        | DynaFn::BIT_SHIFT_LEFT
        | DynaFn::BIT_SHIFT_RIGHT
        | DynaFn::BIT_XOR
        | DynaFn::BOOLAND
        | DynaFn::BOOLOR
        | DynaFn::CAST
        | DynaFn::CAST_BOOLEAN
        | DynaFn::CBRT
        | DynaFn::CEILING
        | DynaFn::CHAR
        | DynaFn::CHR
        | DynaFn::COALESCE
        | DynaFn::CONTAINS
        | DynaFn::CORR
        | DynaFn::COS
        | DynaFn::COSH
        | DynaFn::COT
        | DynaFn::COUNT
        | DynaFn::COVAR_POPULATION
        | DynaFn::COVAR_SAMPLE
        | DynaFn::CUMULATIVE_DISTRIBUTION
        | DynaFn::CURRENT_USER_ID
        | DynaFn::DATE
        | DynaFn::DATE_DIFF
        | DynaFn::DATE_PART
        | DynaFn::DAY_OF_MWEEK
        | DynaFn::DAY_OF_MONTH
        | DynaFn::DAY_OF_YEAR
        | DynaFn::DECODE_BASE64
        | DynaFn::DENSE_RANK
        | DynaFn::DISTINCT
        | DynaFn::DIVIDE
        | DynaFn::DIVIDE_ROUND
        | DynaFn::ENCODE_BASE64
        | DynaFn::ENDS_WITH
        | DynaFn::EQUAL
        | DynaFn::EXISTS
        | DynaFn::EXP
        | DynaFn::FIRST
        | DynaFn::FIRST_DAY_OF_MONTH
        | DynaFn::FIRST_DAY_OF_QUARTER
        | DynaFn::FIRST_DAY_OF_THIS_MONTH
        | DynaFn::FIRST_DAY_OF_THIS_QUARTER
        | DynaFn::FIRST_DAY_OF_THIS_YEAR
        | DynaFn::FIRST_DAY_OF_WEEK
        | DynaFn::FIRST_DAY_OF_YEAR
        | DynaFn::FIRST_HOUR_OF_DAY
        | DynaFn::FIRST_MILLISECOND_OF_SECOND
        | DynaFn::FIRST_MINUTE_OF_HOUR
        | DynaFn::FIRST_SECOND_OF_MINUTE
        | DynaFn::FLOOR
        | DynaFn::FORMAT_DATE
        | DynaFn::GENERATE_GUID
        | DynaFn::GREATER_THAN
        | DynaFn::GREATER_THAN_EQUAL
        | DynaFn::GREATEST
        | DynaFn::HASH_AGG
        | DynaFn::HASH_CODE
        | DynaFn::HOUR
        | DynaFn::IN
        | DynaFn::IS_ALPHA_NUMERIC
        | DynaFn::IS_DISTINCT
        | DynaFn::IS_EMPTY
        | DynaFn::IS_NOT_EMPTY
        | DynaFn::IS_NUMERIC
        | DynaFn::JARO_WINKLER_SIMILARITY
        | DynaFn::JOIN_STRINGS
        | DynaFn::KEYS
        | DynaFn::LAG
        | DynaFn::LAST
        | DynaFn::LEAD
        | DynaFn::LEAST
        | DynaFn::LEFT
        | DynaFn::LENGTH
        | DynaFn::LESS_THAN
        | DynaFn::LESS_THAN_EQUAL
        | DynaFn::LEVENSHTEIN_DISTANCE
        | DynaFn::LOG
        | DynaFn::LOG10
        | DynaFn::LPAD
        | DynaFn::LTRIM
        | DynaFn::MAP_CONCATENATE
        | DynaFn::MATCHES
        | DynaFn::MAX
        | DynaFn::MAX_BY
        | DynaFn::MEDIAN
        | DynaFn::MIN
        | DynaFn::MIN_BY
        | DynaFn::MINUS
        | DynaFn::MINUTE
        | DynaFn::MOD
        | DynaFn::MODE
        | DynaFn::MONTH
        | DynaFn::MONTH_NAME
        | DynaFn::MONTH_NUMBER
        | DynaFn::MOST_RECENT_DAY_OF_WEEK
        | DynaFn::NOT
        | DynaFn::NOT_EQUAL
        | DynaFn::NOT_EQUAL_ANSI
        | DynaFn::NOW
        | DynaFn::NTH
        | DynaFn::NTILE
        | DynaFn::NULL_SAFE_EQUAL
        | DynaFn::NULL_SAFE_NOT_EQUAL
        | DynaFn::OBJECT_REFERENCE_IN
        | DynaFn::PARSE_BOOLEAN
        | DynaFn::PARSE_DECIMAL
        | DynaFn::PARSE_FLOAT
        | DynaFn::PARSE_INTEGER
        | DynaFn::PARSE_JSON
        | DynaFn::PERCENT_RANK
        | DynaFn::PERCENTILE
        | DynaFn::PLUS
        | DynaFn::POW
        | DynaFn::PREVIOUS_DAY_OF_WEEK
        | DynaFn::QUARTER
        | DynaFn::QUARTER_NUMBER
        | DynaFn::RANGE
        | DynaFn::RANK
        | DynaFn::REGEXP_COUNT
        | DynaFn::REGEXP_EXTRACT
        | DynaFn::REGEXP_INDEX_OF
        | DynaFn::REGEXP_LIKE
        | DynaFn::REGEXP_REPLACE
        | DynaFn::REM
        | DynaFn::REPEAT_STRING
        | DynaFn::REPLACE
        | DynaFn::REVERSE
        | DynaFn::REVERSE_STRING
        | DynaFn::RIGHT
        | DynaFn::ROUND
        | DynaFn::ROW_NUMBER
        | DynaFn::RPAD
        | DynaFn::RTRIM
        | DynaFn::SECOND
        | DynaFn::SIGN
        | DynaFn::SIN
        | DynaFn::SINH
        | DynaFn::SIZE
        | DynaFn::SPLIT
        | DynaFn::SQL_FALSE
        | DynaFn::SQL_NULL
        | DynaFn::SQL_TRUE
        | DynaFn::SQRT
        | DynaFn::STARTS_WITH
        | DynaFn::STD_DEV_POPULATION
        | DynaFn::STD_DEV_SAMPLE
        | DynaFn::SUM
        | DynaFn::TAN
        | DynaFn::TANH
        | DynaFn::TIME_BUCKET
        | DynaFn::TIMES
        | DynaFn::TO_DECIMAL
        | DynaFn::TO_FLOAT
        | DynaFn::TO_JSON
        | DynaFn::TO_LOWER
        | DynaFn::TO_ONE
        | DynaFn::TO_STRING
        | DynaFn::TO_UPPER
        | DynaFn::TO_VARIANT
        | DynaFn::TO_VARIANT_LIST
        | DynaFn::TO_VARIANT_OBJECT
        | DynaFn::TODAY
        | DynaFn::TRIM
        | DynaFn::VALUES
        | DynaFn::VARIANCE
        | DynaFn::VARIANCE_POPULATION
        | DynaFn::VARIANCE_SAMPLE
        | DynaFn::VARIANT_TO
        | DynaFn::WEEK_OF_YEAR
        | DynaFn::YEAR => None,
    }
}

/// `DynaFnArms.ARMS`: every engine dynafunction with a translator arm, in the enum's order.
pub fn arms() -> Vec<DynaFn> {
    DynaFn::values().filter(|d| arm(*d).is_some()).collect()
}

/// `DynaFnArms.LANDINGS`: each FORMAT arm's dynafunction and the `Pure.Lite` identity it lands on, in the enum's order.
pub fn landings() -> Vec<(DynaFn, &'static str)> {
    DynaFn::values()
        .filter_map(|d| arm(d).and_then(Arm::landing).map(|l| (d, l)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_builtin::dyna_fn_decisions::Resolution;
    use std::collections::BTreeSet;

    #[test]
    fn the_arm_set_is_javas() {
        let names: Vec<&str> = arms().iter().map(|d| d.dyna_name()).collect();
        let java: Vec<DynaFn> = vec![
            DynaFn::ADD,
            DynaFn::ADJUST,
            DynaFn::AND,
            DynaFn::CASE,
            DynaFn::CONCAT,
            DynaFn::CONVERT_DATE,
            DynaFn::CONVERT_DATE_TIME,
            DynaFn::CONVERT_TIME_ZONE,
            DynaFn::CONVERT_VARCHAR128,
            DynaFn::DAY_OF_WEEK,
            DynaFn::DAY_OF_WEEK_NUMBER,
            DynaFn::EXTRACT_FROM_SEMI_STRUCTURED,
            DynaFn::GROUP,
            DynaFn::IF,
            DynaFn::INDEX_OF,
            DynaFn::IS_NOT_NULL,
            DynaFn::IS_NULL,
            DynaFn::MD5,
            DynaFn::OR,
            DynaFn::PARSE_DATE,
            DynaFn::POSITION,
            DynaFn::SHA1,
            DynaFn::SHA256,
            DynaFn::SPLIT_PART,
            DynaFn::SUB,
            DynaFn::SUBSTRING,
            DynaFn::TO_TIMESTAMP,
        ];
        let mut ours = arms();
        ours.sort();
        let mut theirs = java.clone();
        theirs.sort();
        assert_eq!(ours, theirs, "{names:?}");
        for d in java {
            assert_eq!(arm(d).map(Arm::dyna_fn), Some(d));
        }
    }

    #[test]
    fn the_landings_are_javas() {
        let mut got: Vec<(DynaFn, &str)> = landings();
        got.sort();
        let mut want = vec![
            (DynaFn::PARSE_DATE, lite::PARSE_DATE_FORMAT),
            (DynaFn::CONVERT_DATE, lite::CONVERT_DATE_FORMAT),
            (DynaFn::CONVERT_DATE_TIME, lite::CONVERT_DATE_TIME_FORMAT),
            (DynaFn::TO_TIMESTAMP, lite::CONVERT_DATE_TIME_FORMAT),
            (DynaFn::CONVERT_TIME_ZONE, lite::CONVERT_TIME_ZONE_FORMAT),
        ];
        want.sort();
        assert_eq!(got, want);
    }

    /// `DynaFnRegistryTest.resolutionsHold`'s two arm rules.
    #[test]
    fn translated_names_are_armed_and_armed_names_are_translated_or_pure() {
        for d in DynaFn::with_resolution(Resolution::TRANSLATED) {
            assert!(
                arm(d).is_some(),
                "{}: TRANSLATED but the translator declares no arm for it",
                d.dyna_name()
            );
        }
        for d in arms() {
            let r = d.resolution();
            assert!(
                r == Resolution::TRANSLATED || r == Resolution::PURE,
                "{}: has a translator arm but is {}",
                d.dyna_name(),
                r.name()
            );
        }
    }

    /// `DynaFnRegistryTest.engineVocabShimsAreDerivedFromTheRegistry`.
    #[test]
    fn the_engine_vocabulary_shims_are_the_shim_rows_and_the_landings() {
        let mut derived: BTreeSet<String> = BTreeSet::new();
        for d in DynaFn::with_resolution(Resolution::SHIM) {
            let fqn = d.lite_fqn().expect("a SHIM names its Lite native");
            derived.insert(fqn.strip_prefix(lite::PKG).expect("a Lite FQN").to_owned());
        }
        for (_, fqn) in landings() {
            derived.insert(fqn.strip_prefix(lite::PKG).expect("a Lite FQN").to_owned());
        }
        let shims: BTreeSet<String> = lite_builtin::pure::ENGINE_VOCAB_SHIMS
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert_eq!(derived, shims);
    }
}
