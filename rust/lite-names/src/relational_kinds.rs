// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `RelationalKinds` (2026-11-12): a column's SQL type to its Pure kind, exhaustive. A type Pure cannot name is a
//! `String` (read as text); a nested value is a `Variant`.

use lite_model::RelationalDataType as T;

/// `pureKindOf(t)`.
pub fn pure_kind_of(t: &T) -> &'static str {
    match t {
        T::RelationalDataTypeVarchar(_) | T::RelationalDataTypeChar_(_) => "String",
        T::RelationalDataTypeBigInt(_)
        | T::RelationalDataTypeSmallInt(_)
        | T::RelationalDataTypeTinyInt(_)
        | T::RelationalDataTypeInteger_(_) => "Integer",
        T::RelationalDataTypeFloat_(_) | T::RelationalDataTypeDouble_(_) | T::RelationalDataTypeReal(_) => "Float",
        T::RelationalDataTypeDecimal(_) | T::RelationalDataTypeNumeric(_) => "Decimal",
        T::RelationalDataTypeBit(_) => "Boolean",
        T::RelationalDataTypeTimestamp(_) | T::RelationalDataTypeTimestampNs(_) => "DateTime",
        T::RelationalDataTypeDate_(_) => "StrictDate",
        T::RelationalDataTypeTime(_) => "StrictTime",
        T::RelationalDataTypeBinary(_) | T::RelationalDataTypeVarbinary(_) => "Byte",
        T::RelationalDataTypeSemiStructured(_) => "Variant",
        T::RelationalDataTypeDistinct(_) | T::RelationalDataTypeOther(_) => "String",
        T::RelationalDataTypeArray(_) | T::RelationalDataTypeObject_(_) => "Variant",
    }
}
