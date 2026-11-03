// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Java enums the MIR names, each with its constants in declaration order and the exact `name()` of each: what
//! MirJson writes for a constant, and the one spelling its reader takes.

/// An enum whose constants are a Java enum's: `as_str` is the constant's `name()`, `from_name` its `valueOf`.
macro_rules! java_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $java:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every constant, in Java's declaration order (`values()`).
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The constant's Java `name()`.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $java),+
                }
            }

            /// The constant whose Java `name()` is `name`.
            pub fn from_name(name: &str) -> Option<$name> {
                match name {
                    $($java => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

java_enum! {
    /// Java's `SqlFn`: the semantic functions a `Call` names.
    SqlFn {
        And => "AND",
        Or => "OR",
        Not => "NOT",
        Equal => "EQUAL",
        NotEqual => "NOT_EQUAL",
        Less => "LESS",
        LessEqual => "LESS_EQUAL",
        Greater => "GREATER",
        GreaterEqual => "GREATER_EQUAL",
        Plus => "PLUS",
        Minus => "MINUS",
        Times => "TIMES",
        Divide => "DIVIDE",
        Mod => "MOD",
        Rem => "REM",
        Negate => "NEGATE",
        Abs => "ABS",
        IsNull => "IS_NULL",
        IsNotNull => "IS_NOT_NULL",
        In => "IN",
        Coalesce => "COALESCE",
        ParseInt => "PARSE_INT",
        Concat => "CONCAT",
        Length => "LENGTH",
        Upper => "UPPER",
        Lower => "LOWER",
        Greatest => "GREATEST",
        Least => "LEAST",
        ConcatJoin => "CONCAT_JOIN",
        ListFilter => "LIST_FILTER",
        ListTransform => "LIST_TRANSFORM",
        ListConcat => "LIST_CONCAT",
        ListGet => "LIST_GET",
        ListPosition => "LIST_POSITION",
        ListExists => "LIST_EXISTS",
        ListForAll => "LIST_FOR_ALL",
        StructInsert => "STRUCT_INSERT",
        Unnest => "UNNEST",
        ListFlatten => "LIST_FLATTEN",
        JsonMergePatch => "JSON_MERGE_PATCH",
        Sqrt => "SQRT",
        Cbrt => "CBRT",
        Exp => "EXP",
        Ln => "LN",
        Log10 => "LOG10",
        Pow => "POW",
        Pi => "PI",
        Sin => "SIN",
        Cos => "COS",
        Tan => "TAN",
        Asin => "ASIN",
        Acos => "ACOS",
        Atan => "ATAN",
        Atan2 => "ATAN2",
        Sinh => "SINH",
        Cosh => "COSH",
        Tanh => "TANH",
        Ceiling => "CEILING",
        Floor => "FLOOR",
        Round => "ROUND",
        Sign => "SIGN",
        Xor => "XOR",
        BitAnd => "BIT_AND",
        BitOr => "BIT_OR",
        BitXor => "BIT_XOR",
        BitShiftLeft => "BIT_SHIFT_LEFT",
        BitShiftRight => "BIT_SHIFT_RIGHT",
        BitNot => "BIT_NOT",
        RegexpExtract => "REGEXP_EXTRACT",
        RegexpExtractAll => "REGEXP_EXTRACT_ALL",
        RegexpReplace => "REGEXP_REPLACE",
        RegexpFullMatch => "REGEXP_FULL_MATCH",
        MapFromLists => "MAP_FROM_LISTS",
        MapFromEntries => "MAP_FROM_ENTRIES",
        MapEmpty => "MAP_EMPTY",
        MapExtract => "MAP_EXTRACT",
        MapKeys => "MAP_KEYS",
        MapValues => "MAP_VALUES",
        MapConcat => "MAP_CONCAT",
        Substring => "SUBSTRING",
        Strpos => "STRPOS",
        StartsWith => "STARTS_WITH",
        EndsWith => "ENDS_WITH",
        Matches => "MATCHES",
        Left => "LEFT",
        Right => "RIGHT",
        Lpad => "LPAD",
        Rpad => "RPAD",
        Trim => "TRIM",
        Ltrim => "LTRIM",
        Rtrim => "RTRIM",
        Replace => "REPLACE",
        Split => "SPLIT",
        SplitPart => "SPLIT_PART",
        Translate => "TRANSLATE",
        PureSplitPart => "PURE_SPLIT_PART",
        ReverseString => "REVERSE_STRING",
        AsciiCode => "ASCII_CODE",
        Chr => "CHR",
        UcFirst => "UC_FIRST",
        LcFirst => "LC_FIRST",
        EncodeBase64 => "ENCODE_BASE64",
        Levenshtein => "LEVENSHTEIN",
        Guid => "GUID",
        Format => "FORMAT",
        Hash => "HASH",
        Md5 => "MD5",
        Sha1 => "SHA1",
        Sha256 => "SHA256",
        Random => "RANDOM",
        Extract => "EXTRACT",
        Today => "TODAY",
        Now => "NOW",
        DateTruncDay => "DATE_TRUNC_DAY",
        MakeDate => "MAKE_DATE",
        MakeTimestamp => "MAKE_TIMESTAMP",
        DateTrunc => "DATE_TRUNC",
        AddInterval => "ADD_INTERVAL",
        DateDiff => "DATE_DIFF",
        TimeBucket => "TIME_BUCKET",
        Timezone => "TIMEZONE",
        DateAtTime => "DATE_AT_TIME",
        EpochNs => "EPOCH_NS",
        FromEpochNs => "FROM_EPOCH_NS",
        AddIntervalTemporal => "ADD_INTERVAL_TEMPORAL",
        EpochSeconds => "EPOCH_SECONDS",
        EpochMs => "EPOCH_MS",
        FromEpochSeconds => "FROM_EPOCH_SECONDS",
        FromEpochMs => "FROM_EPOCH_MS",
        Dayname => "DAYNAME",
        Monthname => "MONTHNAME",
        Cot => "COT",
        Radians => "RADIANS",
        Degrees => "DEGREES",
        IntDivide => "INT_DIVIDE",
        RepeatStr => "REPEAT_STR",
        JaroWinkler => "JARO_WINKLER",
        DecodeBase64 => "DECODE_BASE64",
        CurrentUserFn => "CURRENT_USER_FN",
        ListLength => "LIST_LENGTH",
        ListZip => "LIST_ZIP",
        ListDistinct => "LIST_DISTINCT",
        ListAppend => "LIST_APPEND",
        ListSum => "LIST_SUM",
        ListMin => "LIST_MIN",
        ListMax => "LIST_MAX",
        ListAvg => "LIST_AVG",
        ListMedian => "LIST_MEDIAN",
        ListMode => "LIST_MODE",
        ListSort => "LIST_SORT",
        ListSortDesc => "LIST_SORT_DESC",
        ListTail => "LIST_TAIL",
        ListInit => "LIST_INIT",
        RangeFn => "RANGE_FN",
        ListProduct => "LIST_PRODUCT",
        ListReduce => "LIST_REDUCE",
        ListSlice => "LIST_SLICE",
        RepeatValue => "REPEAT_VALUE",
        ListBoolAnd => "LIST_BOOL_AND",
        ListBoolOr => "LIST_BOOL_OR",
        IsDistinctFrom => "IS_DISTINCT_FROM",
        AllDistinct => "ALL_DISTINCT",
        NullSafeEqual => "NULL_SAFE_EQUAL",
        NullSafeNotEqual => "NULL_SAFE_NOT_EQUAL",
        Strftime => "STRFTIME",
        Strptime => "STRPTIME",
        ParseDate => "PARSE_DATE",
        ListReverse => "LIST_REVERSE",
        Typeof => "TYPEOF",
        RoundHalfUp => "ROUND_HALF_UP",
        Error => "ERROR",
        FloorRaw => "FLOOR_RAW",
        ToVariant => "TO_VARIANT",
        BoolToText => "BOOL_TO_TEXT",
        FloatCanon => "FLOAT_CANON",
        FloatPrint => "FLOAT_PRINT",
        VariantElements => "VARIANT_ELEMENTS",
        VariantGet => "VARIANT_GET",
        JsonType => "JSON_TYPE",
        JsonArrayLength => "JSON_ARRAY_LENGTH",
        JsonPretty => "JSON_PRETTY",
    }
}

java_enum! {
    /// Java's `SqlAgg.Fn`: the aggregate and window functions.
    AggFn {
        Sum => "SUM",
        Count => "COUNT",
        Avg => "AVG",
        Min => "MIN",
        Max => "MAX",
        AnyValue => "ANY_VALUE",
        StddevSamp => "STDDEV_SAMP",
        StddevPop => "STDDEV_POP",
        VarSamp => "VAR_SAMP",
        VarPop => "VAR_POP",
        Median => "MEDIAN",
        Mode => "MODE",
        StringAgg => "STRING_AGG",
        List => "LIST",
        QuantileCont => "QUANTILE_CONT",
        QuantileDisc => "QUANTILE_DISC",
        Corr => "CORR",
        CovarSamp => "COVAR_SAMP",
        CovarPop => "COVAR_POP",
        ArgMax => "ARG_MAX",
        ArgMin => "ARG_MIN",
        Wavg => "WAVG",
        HashList => "HASH_LIST",
        BoolAnd => "BOOL_AND",
        BoolOr => "BOOL_OR",
        IsDistinctMark => "IS_DISTINCT_MARK",
        UniqueValueOnly => "UNIQUE_VALUE_ONLY",
        Variance => "VARIANCE",
        Stddev => "STDDEV",
        RowNumber => "ROW_NUMBER",
        Rank => "RANK",
        DenseRank => "DENSE_RANK",
        PercentRank => "PERCENT_RANK",
        CumeDist => "CUME_DIST",
        Ntile => "NTILE",
        Lag => "LAG",
        Lead => "LEAD",
        FirstValue => "FIRST_VALUE",
        LastValue => "LAST_VALUE",
        NthValue => "NTH_VALUE",
    }
}

java_enum! {
    /// Java's `SqlType.Scalar`: the scalar SQL types.
    Scalar {
        Boolean => "BOOLEAN",
        Integer => "INTEGER",
        Bigint => "BIGINT",
        Hugeint => "HUGEINT",
        Double => "DOUBLE",
        Varchar => "VARCHAR",
        Date => "DATE",
        Timestamp => "TIMESTAMP",
        Timestamptz => "TIMESTAMPTZ",
        Time => "TIME",
        Json => "JSON",
        TemporalText => "TEMPORAL_TEXT",
        DecimalText => "DECIMAL_TEXT",
    }
}

java_enum! {
    /// Java's `DateFmt.Part`: a date-format directive.
    DateFmtPart {
        Year4 => "YEAR4",
        Month2 => "MONTH2",
        Day2 => "DAY2",
        Hour2 => "HOUR2",
        Min2 => "MIN2",
        Sec2 => "SEC2",
        SubsecMicro => "SUBSEC_MICRO",
        SubsecNano => "SUBSEC_NANO",
        SubsecMin => "SUBSEC_MIN",
        MonthAbbrev => "MONTH_ABBREV",
        MonthName => "MONTH_NAME",
        WeekdayName => "WEEKDAY_NAME",
        Hour12 => "HOUR12",
        Hour12Nopad => "HOUR12_NOPAD",
        Ampm => "AMPM",
    }
}

java_enum! {
    /// Java's `DatePart`: a date or time part.
    DatePart {
        Year => "YEAR",
        Quarter => "QUARTER",
        Month => "MONTH",
        Week => "WEEK",
        Day => "DAY",
        Hour => "HOUR",
        Minute => "MINUTE",
        Second => "SECOND",
        Millisecond => "MILLISECOND",
        Microsecond => "MICROSECOND",
        Nanosecond => "NANOSECOND",
        DayOfYear => "DAY_OF_YEAR",
        IsoDayOfWeek => "ISO_DAY_OF_WEEK",
    }
}

java_enum! {
    /// Java's `JsonKind`: a JSON value's kind.
    JsonKind {
        Object => "OBJECT",
        Array => "ARRAY",
        Null => "NULL",
        String => "STRING",
        Integer => "INTEGER",
        UnsignedInteger => "UNSIGNED_INTEGER",
        Float => "FLOAT",
        Boolean => "BOOLEAN",
    }
}

java_enum! {
    /// Java's `ValueKind`: a kind of value a value-typed parameter takes.
    ValueKind {
        Integer => "INTEGER",
        Decimal => "DECIMAL",
        Floating => "FLOATING",
        Date => "DATE",
        DateTime => "DATE_TIME",
        DateTimeNanos => "DATE_TIME_NANOS",
    }
}

java_enum! {
    /// Java's `OutputCol.Origin`: where a column name was born.
    Origin {
        Physical => "PHYSICAL",
        PhysicalQuoted => "PHYSICAL_QUOTED",
        Derived => "DERIVED",
        PerKey => "PER_KEY",
    }
}

java_enum! {
    /// Java's `SqlSelect.SortKey.NullOrder`.
    NullOrder {
        NullsFirst => "NULLS_FIRST",
        NullsLast => "NULLS_LAST",
    }
}

java_enum! {
    /// Java's `SqlSource.Join.Kind`.
    JoinKind {
        Inner => "INNER",
        Left => "LEFT",
        Right => "RIGHT",
        Full => "FULL",
        Cross => "CROSS",
        CrossLateral => "CROSS_LATERAL",
        AsofLeft => "ASOF_LEFT",
        LeftLateral => "LEFT_LATERAL",
    }
}

java_enum! {
    /// Java's `SqlSource.Catalog.Grid`.
    CatalogGrid {
        Schemas => "SCHEMAS",
        Tables => "TABLES",
        Columns => "COLUMNS",
        PrimaryKeys => "PRIMARY_KEYS",
    }
}

java_enum! {
    /// Java's `SqlExpr.PlanParam.Kind`.
    ParamKind {
        String => "STRING",
        Date => "DATE",
        StrictDate => "STRICT_DATE",
        Datetime => "DATETIME",
        Float => "FLOAT",
        Boolean => "BOOLEAN",
        Enum => "ENUM",
        Other => "OTHER",
        Raw => "RAW",
    }
}

java_enum! {
    /// Java's `SqlExpr.Quantifier`.
    Quantifier {
        Any => "ANY",
        All => "ALL",
    }
}

java_enum! {
    /// Java's `SqlExpr.DeferredTdsString.Form`.
    TdsForm {
        TdsString => "TDS_STRING",
        Csv => "CSV",
    }
}

java_enum! {
    /// Java's `SqlExpr.WindowCall.Frame.Kind`.
    FrameKind {
        Rows => "ROWS",
        Range => "RANGE",
    }
}

java_enum! {
    /// Java's `SqlDdl.ColumnType.Kind`: a plain declared column type.
    ColumnKind {
        Bigint => "BIGINT",
        Smallint => "SMALLINT",
        Tinyint => "TINYINT",
        Integer => "INTEGER",
        Float => "FLOAT",
        Double => "DOUBLE",
        Real => "REAL",
        Bit => "BIT",
        Timestamp => "TIMESTAMP",
        Date => "DATE",
        Time => "TIME",
        TimestampNs => "TIMESTAMP_NS",
        Json => "JSON",
        Varchar => "VARCHAR",
        Other => "OTHER",
        Distinct => "DISTINCT",
        Array => "ARRAY",
        Object => "OBJECT",
    }
}
