// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `StoreCompiler.tableSchema` (StoreCompiler.java:30 at 18feff3fe): a database's physical table as its Pure row
//! struct, the bare relation type `#>{db.TABLE}#` is typed by. Its column types are THE store column type,
//! [`lite_names::store_compiler::column_type`] (`StoreCompiler.columnType`, moved to lite-names with its first caller,
//! E's `ViewSignatures`: the owner's ruling of 2026-11-12). `declaredType` (the SQL layer's declared type) is G's,
//! not here (EF_CENSUS §2, ruling 27).

use lite_base::Alloc;
use lite_model::DatabaseDefinitionTableDefinition;
use lite_names::Thrown;
use lite_names::store_compiler::column_type;
use lite_types::{Column, Multiplicity, RelationType};

/// `tableSchema(table)` (:34): the table's columns in declaration order, each by its bare name with its column type,
/// `[1]` when `NOT NULL` or `PRIMARY KEY`, else `[0..1]`; in `arena`. A DECIMAL or NUMERIC whose scale is not in
/// [0, precision], and a column declared twice, are refused by name, a `ModelException` (MODEL) naming the table and
/// the column, each at its column in declaration order (ruling 29's policy 2, 2026-11-16, the typer slice's stage 1a
/// item 5: the precise decimal's and the relation type's raw `IllegalArgumentException`s until then); a type with no
/// Pure spelling is `columnType`'s refusal, as Java lets it escape.
pub fn table_schema<'a, A: Alloc>(
    arena: &'a A,
    table: &DatabaseDefinitionTableDefinition,
) -> Result<&'a RelationType<'a>, Thrown> {
    let mut columns = Vec::with_capacity(table.columns.len());
    let mut seen = std::collections::HashSet::new();
    for col in table.columns.iter() {
        let bad_scale = match &col.data_type {
            lite_model::RelationalDataType::RelationalDataTypeDecimal(d) => !scale_fits(d.precision, d.scale),
            lite_model::RelationalDataType::RelationalDataTypeNumeric(n) => !scale_fits(n.precision, n.scale),
            _ => false,
        };
        if bad_scale {
            return Err(Thrown::model(
                lite_error::Phase::Model,
                lite_model::JStr::from(format!(
                    "table '{}': the column '{}' declares a precision and scale its type cannot hold (the scale must \
                     be in [0, precision])",
                    table.name.lossy(),
                    col.name.lossy()
                )),
                None,
            ));
        }
        if !seen.insert(col.name.clone()) {
            return Err(Thrown::model(
                lite_error::Phase::Model,
                lite_model::JStr::from(format!(
                    "table '{}' declares the column '{}' twice",
                    table.name.lossy(),
                    col.name.lossy()
                )),
                None,
            ));
        }
        let multiplicity = if col.not_null || col.primary_key {
            Multiplicity::ONE
        } else {
            Multiplicity::ZERO_ONE
        };
        // the BARE name: relation space names columns as Pure does (the engine strips a declaration's quotes where a
        // table becomes a relation). Quoting is a spelling, carried to the SQL as a rendering fact
        // (TypedTableReference.quotedColumns). The name goes into `arena` as Rust text (`JStr::lossy`), as lite-names'
        // `ViewSignatures` puts a view's column names
        columns.push(Column {
            name: arena.alloc_str(&col.name.lossy()),
            r#type: column_type(&col.data_type)?,
            multiplicity,
        });
    }
    RelationType::of(arena, &columns).map_err(Thrown::from_type_error)
}

/// `scaleFits(precision, scale)` (:85): `Type.PrecisionDecimal`'s own check, a precision of at least 0 and a scale in
/// [0, precision].
fn scale_fits(precision: i32, scale: i32) -> bool {
    precision >= 0 && scale >= 0 && scale <= precision
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use lite_base::QueryArena;
    use lite_model::text::j;
    use lite_model::{
        DatabaseDefinitionColumnDefinition, DatabaseDefinitionTableDefinition, RelationalDataType as T,
        RelationalDataTypeArray, RelationalDataTypeDecimal, RelationalDataTypeDistinct, RelationalDataTypeInteger_,
        RelationalDataTypeObject_, RelationalDataTypeOther, RelationalDataTypeSemiStructured,
        RelationalDataTypeVarchar,
    };
    use lite_names::Thrown;
    use lite_types::{Multiplicity, Primitive, Type, platform};

    use super::table_schema;

    fn integer() -> T {
        T::RelationalDataTypeInteger_(Arc::new(RelationalDataTypeInteger_ {}))
    }

    fn varchar(size: i64) -> T {
        T::RelationalDataTypeVarchar(Arc::new(RelationalDataTypeVarchar { size }))
    }

    fn decimal(precision: i32, scale: i32) -> T {
        T::RelationalDataTypeDecimal(Arc::new(RelationalDataTypeDecimal { precision, scale }))
    }

    fn col(name: &str, data_type: T, primary_key: bool, not_null: bool) -> Arc<DatabaseDefinitionColumnDefinition> {
        Arc::new(DatabaseDefinitionColumnDefinition {
            name: j(name),
            data_type,
            primary_key,
            not_null,
            quoted: false,
        })
    }

    fn table(columns: Vec<Arc<DatabaseDefinitionColumnDefinition>>) -> DatabaseDefinitionTableDefinition {
        DatabaseDefinitionTableDefinition {
            name: j("T"),
            columns: columns.into(),
            milestoning: None,
            function: false,
            quoted: false,
            schema_quoted: false,
        }
    }

    fn assert_model(thrown: Thrown, message: &str) {
        assert!(thrown.is_model(), "{thrown:?}");
        assert_eq!(thrown.message().expect("a message").lossy(), message);
    }

    #[test]
    fn a_tables_columns_are_its_row_struct_in_declaration_order() {
        let cx = QueryArena::new();
        let t = table(vec![
            col("id", integer(), true, false),
            col("name", varchar(20), false, true),
            col("note", varchar(10), false, false),
            col("price", decimal(10, 2), false, false),
            // a quoted declaration's name is the bare one the record holds (its quoting a rendering fact)
            Arc::new(DatabaseDefinitionColumnDefinition {
                name: j("first name"),
                data_type: varchar(5),
                primary_key: false,
                not_null: false,
                quoted: true,
            }),
        ]);
        let schema = table_schema(&cx, &t).unwrap();
        assert_eq!(
            schema.columns().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            vec![
                "id:Integer[1]",
                "name:String[1]",
                "note:String[0..1]",
                "price:Decimal(10,2)[0..1]",
                "first name:String[0..1]"
            ]
        );
        assert!(schema.dynamic_columns().is_empty());
        assert_eq!(schema.columns()[0].multiplicity, Multiplicity::ONE);
        assert_eq!(schema.columns()[2].multiplicity, Multiplicity::ZERO_ONE);
        assert!(table_schema(&cx, &table(vec![])).unwrap().columns().is_empty());
    }

    #[test]
    fn every_store_type_is_the_column_types() {
        // StoreCompilerTypesTest: a type Pure cannot name is a String, a nested value a Variant
        let cx = QueryArena::new();
        let variant = Type::ClassType(platform::VARIANT);
        let t = table(vec![
            col(
                "o",
                T::RelationalDataTypeOther(Arc::new(RelationalDataTypeOther {})),
                false,
                false,
            ),
            col(
                "d",
                T::RelationalDataTypeDistinct(Arc::new(RelationalDataTypeDistinct {})),
                false,
                false,
            ),
            col(
                "s",
                T::RelationalDataTypeSemiStructured(Arc::new(RelationalDataTypeSemiStructured {})),
                false,
                false,
            ),
            col(
                "a",
                T::RelationalDataTypeArray(Arc::new(RelationalDataTypeArray { element: varchar(8) })),
                false,
                false,
            ),
            col(
                "j",
                T::RelationalDataTypeObject_(Arc::new(RelationalDataTypeObject_ {
                    key_type: varchar(8),
                    value_type: integer(),
                })),
                false,
                false,
            ),
        ]);
        let types: Vec<Type<'_>> = table_schema(&cx, &t)
            .unwrap()
            .columns()
            .iter()
            .map(|c| c.r#type)
            .collect();
        assert_eq!(
            types,
            vec![
                Type::Primitive(Primitive::String),
                Type::Primitive(Primitive::String),
                variant,
                variant,
                variant
            ]
        );
    }

    #[test]
    fn a_tables_refusals_name_the_table_and_the_column() {
        // ruling 29's policy 2 (2026-11-16, the typer slice's stage 1a item 5): until then the relation type's and the
        // precise decimal's raw IllegalArgumentExceptions (a_tables_refusals_are_javas_raw_throws, retired: a
        // rust/RETIRED_TESTS.md row)
        let cx = QueryArena::new();
        // two columns of one name
        assert_model(
            table_schema(
                &cx,
                &table(vec![
                    col("id", integer(), true, false),
                    col("id", varchar(3), false, false),
                ]),
            )
            .unwrap_err(),
            "table 'T' declares the column 'id' twice",
        );
        // a DECIMAL whose scale passes its precision, met at its column before any duplicate
        assert_model(
            table_schema(
                &cx,
                &table(vec![
                    col("id", integer(), false, false),
                    col("p", decimal(2, 5), false, false),
                    col("id", integer(), false, false),
                ]),
            )
            .unwrap_err(),
            "table 'T': the column 'p' declares a precision and scale its type cannot hold (the scale must be in [0, \
             precision])",
        );
    }
}
