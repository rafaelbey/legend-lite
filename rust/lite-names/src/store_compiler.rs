// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! THE store column type (2026-11-12, the names follow-up: Java's `compiler/element/StoreCompiler.columnType`, moved
//! into the names layer by the owner's ruling of 2026-11-12 -- its readers are E's `ViewSignatures` and the element
//! compiler's `tableSchema`, which waits for `lite-element`; `declaredType`, the SQL layer's DDL type, waits for its
//! first caller). The single SQL-type-to-Pure-type boundary (engine `SqlDataType.toGenericType`): integer widths
//! collapse to `Integer`, real and double to `Float`, `DATE` to `StrictDate`, `TIMESTAMP` to `DateTime`; a decimal
//! carries its precision and scale (refused as Java's `PrecisionDecimal` refuses a negative precision or a scale out of
//! range); a type Pure cannot name (`OTHER`, `DISTINCT`) is a `String`; a nested value (`ARRAY`, `OBJECT`,
//! `SEMISTRUCTURED`) a `Variant`.

use lite_model::RelationalDataType as T;
use lite_types::{PrecisionDecimal, Primitive, Type, platform};

use crate::error::Thrown;

/// `StoreCompiler.columnType(dt)`.
pub fn column_type(dt: &T) -> Result<Type<'static>, Thrown> {
    Ok(match dt {
        T::RelationalDataTypeBit(_) => Type::Primitive(Primitive::Boolean),
        T::RelationalDataTypeTinyInt(_) => Type::Primitive(Primitive::Integer),
        T::RelationalDataTypeSmallInt(_) => Type::Primitive(Primitive::Integer),
        T::RelationalDataTypeInteger_(_) => Type::Primitive(Primitive::Integer),
        T::RelationalDataTypeBigInt(_) => Type::Primitive(Primitive::Integer),
        T::RelationalDataTypeFloat_(_) => Type::Primitive(Primitive::Float),
        T::RelationalDataTypeDouble_(_) => Type::Primitive(Primitive::Float),
        T::RelationalDataTypeReal(_) => Type::Primitive(Primitive::Float),
        T::RelationalDataTypeDecimal(d) => {
            Type::PrecisionDecimal(PrecisionDecimal::new(d.precision, d.scale).map_err(Thrown::from_type_error)?)
        }
        T::RelationalDataTypeNumeric(n) => {
            Type::PrecisionDecimal(PrecisionDecimal::new(n.precision, n.scale).map_err(Thrown::from_type_error)?)
        }
        T::RelationalDataTypeVarchar(_) => Type::Primitive(Primitive::String),
        T::RelationalDataTypeChar_(_) => Type::Primitive(Primitive::String),
        T::RelationalDataTypeBinary(_) => Type::Primitive(Primitive::Byte),
        T::RelationalDataTypeVarbinary(_) => Type::Primitive(Primitive::Byte),
        T::RelationalDataTypeDate_(_) => Type::Primitive(Primitive::StrictDate),
        T::RelationalDataTypeTimestamp(_) => Type::Primitive(Primitive::DateTime),
        T::RelationalDataTypeTime(_) => Type::Primitive(Primitive::StrictTime),
        T::RelationalDataTypeTimestampNs(_) => Type::Primitive(Primitive::DateTime),
        T::RelationalDataTypeDistinct(_) => Type::Primitive(Primitive::String),
        T::RelationalDataTypeOther(_) => Type::Primitive(Primitive::String),
        // nested values are Variant -- the get()/to(@Type) navigation surface (engine GetChecker's source shape)
        T::RelationalDataTypeSemiStructured(_) => Type::ClassType(platform::VARIANT),
        T::RelationalDataTypeArray(_) => Type::ClassType(platform::VARIANT),
        T::RelationalDataTypeObject_(_) => Type::ClassType(platform::VARIANT),
    })
}
