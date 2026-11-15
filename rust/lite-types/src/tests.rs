// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

// The family's shape: covariance (a model's types are a query's), Copy, Send and Sync, both arenas.

use crate::{
    Column, ExprType, FunctionType, GenericType, Multiplicity, Param, Primitive, RelationType, Type, platform,
};
use lite_base::{Alloc, ModelArena, QueryArena, same};

/// THE PROOF OF COVARIANCE: this compiles only if `Type<'a>` is covariant in `'a`, so a model's type, borrowed for
/// the model's life, shortens to a query's with no conversion (PHASE3_SHIM_DESIGN.md §7.3 c).
fn shorten<'q, 'm: 'q>(t: &'m Type<'m>) -> &'q Type<'q> {
    t
}

fn shorten_column<'q, 'm: 'q>(c: Column<'m>) -> Column<'q> {
    c
}

const fn assert_copy_send_sync<T: Copy + Send + Sync>() {}
const _: () = assert_copy_send_sync::<Type<'static>>();
const _: () = assert_copy_send_sync::<ExprType<'static>>();
const _: () = assert_copy_send_sync::<RelationType<'static>>();
// a type is a tag and a reference: three words, as cheap to copy as Java's reference
const _: () = assert!(std::mem::size_of::<Type<'static>>() <= 24);

#[test]
fn a_models_types_are_a_querys_with_no_conversion() {
    let model = ModelArena::new();
    let person = model.alloc_str("my::app::Person");
    let schema = RelationType::of(
        &model,
        &[Column {
            name: "name",
            r#type: Type::Primitive(Primitive::String),
            multiplicity: Multiplicity::ONE,
        }],
    )
    .unwrap();
    let in_model: &Type<'_> = model.alloc(Type::relation(&model, schema));
    {
        let cx = QueryArena::new();
        let local = cx.alloc_str("T");
        // a query's generic type over the model's types and its own: the model's shorten to the query's lifetime
        let mixed = GenericType::new(&cx, platform::PAIR, &[*shorten(in_model), Type::TypeVar(local)], &[]);
        let ft = FunctionType::new(
            &cx,
            &[Param {
                r#type: Type::ClassType(person),
                multiplicity: Multiplicity::ZERO_ONE,
            }],
            Param {
                r#type: Type::GenericType(mixed),
                multiplicity: Multiplicity::ONE,
            },
        );
        let lifted = shorten_column(schema.columns()[0]);
        assert_eq!(lifted.name, "name");
        assert_eq!(
            Type::FunctionType(ft).type_name(),
            "{my::app::Person[0..1] -> Pair<Relation<(name:String[1])>, T>[1]}"
        );
        // the query's record holds the model's schema record itself, not a copy
        let Type::GenericType(held) = mixed.arguments[0] else {
            panic!("a generic type")
        };
        let Type::RelationType(held) = held.arguments[0] else {
            panic!("a relation type")
        };
        assert!(same(&held, &schema));
    }
    // the model's type outlives the query that borrowed it
    assert_eq!(in_model.type_name(), "Relation<(name:String[1])>");
}

#[test]
fn the_late_bound_schema_is_a_static() {
    let grid: &'static RelationType<'static> = RelationType::late_bound();
    assert!(grid.is_late_bound() && !grid.is_erased_row());
    let cx = QueryArena::new();
    let row = RelationType::erased_row(&cx, platform::TDS_ROW);
    assert!(row.is_late_bound() && row.is_erased_row());
    // the static one beside a query's in one slice
    let both = cx.alloc_slice(&[Type::RelationType(grid), Type::RelationType(row)]);
    assert_eq!(both[0], Type::RelationType(RelationType::late_bound()));
}

#[test]
fn erasure_makes_each_row_owner_the_erased_row_and_keeps_every_other_type() {
    let cx = QueryArena::new();
    // both owners of the row-accessor family: TDSRow and the ResultSet's execute::Row
    for owner in [platform::TDS_ROW, lite_builtin::native_fn::EXECUTE_ROW_OWNER] {
        let erased = platform::erase_tds_row(&cx, Type::ClassType(owner));
        let Type::RelationType(row) = erased else {
            panic!("{owner} erases to a relation type")
        };
        assert!(row.is_late_bound() && row.is_erased_row());
        // the template keeps the owner's class
        assert_eq!(row.dynamic_columns()[0].r#type, Type::ClassType(owner));
        assert_eq!(erased, Type::RelationType(RelationType::erased_row(&cx, owner)));
    }
    // a class of no accessor family, a generic over a row owner, a primitive: each is itself
    for kept in [
        Type::ClassType("my::app::Row"),
        Type::ClassType(platform::TABULAR_DATA_SET),
        Type::GenericType(GenericType::new(
            &cx,
            platform::LIST,
            &[Type::ClassType(platform::TDS_ROW)],
            &[],
        )),
        Type::Primitive(Primitive::String),
    ] {
        assert_eq!(platform::erase_tds_row(&cx, kept), kept);
    }
}

#[test]
fn types_are_built_on_many_threads_in_one_model() {
    let model = ModelArena::new();
    let made: Vec<Vec<Type<'_>>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..6)
            .map(|t| {
                let model = &model;
                s.spawn(move || {
                    (0..2000)
                        .map(|i| {
                            let raw = model.alloc_str(&format!("my::t{t}::Box{i}"));
                            Type::GenericType(GenericType::new(
                                model,
                                raw,
                                &[Type::Primitive(Primitive::Integer)],
                                &[],
                            ))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect()
    });
    for (t, types) in made.iter().enumerate() {
        for (i, ty) in types.iter().enumerate() {
            assert_eq!(ty.type_name(), format!("Box{i}<Integer>"));
            assert_eq!(ty.class_fqn(), Some(format!("my::t{t}::Box{i}").as_str()));
        }
    }
}

#[test]
fn a_query_keeps_a_box_sql_tree_its_records_point_at() {
    let cx = QueryArena::new();
    let tree: &lite_sql::SqlType = cx.keep(lite_sql::SqlType::Array {
        element: Box::new(lite_sql::SqlType::Scalar(lite_sql::Scalar::Bigint)),
    });
    assert_eq!(Type::kind_of_sql_type(tree), None);
    assert_eq!(cx.kept(), 1);
}
