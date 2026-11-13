// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! THE signature of a store VIEW as a relation (2026-11-12, the names follow-up: Java's
//! `compiler/element/ViewSignatures`, moved into the names layer by the owner's ruling of 2026-11-12 -- its one reader is
//! E's `LiftedViews`): the declared return type of its lifted function, read off store facts the way the engine types
//! view columns (`inferRelationalType` over `columnMappings`):
//! - a plain column carries its table column's Pure type and its NOT NULL / PRIMARY KEY multiplicity (the table lookup
//!   is the index's, include closure); a plain column of an inner VIEW carries that view's signature column
//!   (view-on-view); a plain column found nowhere is `Any[0..1]`;
//! - an aggregate column carries its reducer's Pure result over the argument's store type (`count` `Integer[1]`; `sum`
//!   and `average` `[1]`; `max` / `min` `[0..1]`; Integer, Float and the dates keep their type, every other number is
//!   Number);
//! - any other expression carries the engine's inferred relational type at `[0..1]`, or `Any` when the engine's own rule
//!   has none;
//! - a column the view declares PRIMARY KEY is `[1]` whatever its source.
//!
//! Ported line for line, Java's refusals included: a view chain that reads its own columns is a `ModelException`
//! (MODEL); two columns of one name are the relation type's `IllegalArgumentException`; a decimal out of range is
//! `PrecisionDecimal`'s. The views being expanded are an identity set, as Java's `IdentityHashMap` set is (step 0's
//! census, `expanding`: two equal views in two schemas are two views). A column's name is the record's, its UTF-16
//! read as UTF-8 for the type family's `&str` (a lone surrogate would not survive: none is met).

use std::sync::Arc;

use lite_base::{Alloc, IdSet};
use lite_error::Phase;
use lite_model::{
    DatabaseDefinition, DatabaseDefinitionViewDefinition, DatabaseDefinitionViewDefinitionViewColumnMapping, JStr,
    RelationalOperation as Op, cat,
};
use lite_types::{Column, Family, Multiplicity, Primitive, RelationType, Type, platform};

use crate::error::Thrown;
use crate::java_string;
use crate::model_index::StoreLookups;
use crate::relational_type_inference;
use crate::store_compiler;

type View = Arc<DatabaseDefinitionViewDefinition>;

/// `ViewSignatures.of(store, dbFqn, view)`: the view's signature, its types in `arena`.
pub fn of<'a, A: Alloc>(
    arena: &'a A,
    store: &dyn StoreLookups,
    db_fqn: &JStr,
    view: &View,
) -> Result<&'a RelationType<'a>, Thrown> {
    of_in(arena, store, db_fqn, view, &mut IdSet::new())
}

fn of_in<'a, A: Alloc>(
    arena: &'a A,
    store: &dyn StoreLookups,
    db_fqn: &JStr,
    view: &View,
    expanding: &mut IdSet<View>,
) -> Result<&'a RelationType<'a>, Thrown> {
    if !expanding.insert(view.clone()) {
        return Err(Thrown::model(
            Phase::Model,
            cat!(
                "view '",
                &view.name,
                "' of '",
                db_fqn,
                "' reads its own columns (cyclic view-on-view chain)"
            ),
            None,
        ));
    }
    // Java's try/finally: the view leaves the set whatever the answer
    let out = signature(arena, store, db_fqn, view, expanding);
    expanding.remove(view);
    out
}

fn signature<'a, A: Alloc>(
    arena: &'a A,
    store: &dyn StoreLookups,
    db_fqn: &JStr,
    view: &View,
    expanding: &mut IdSet<View>,
) -> Result<&'a RelationType<'a>, Thrown> {
    let db = store.find_database(Some(db_fqn)).map(|d| &**d);
    let mut columns: Vec<Column<'a>> = Vec::with_capacity(view.column_mappings.len());
    for cm in view.column_mappings.iter() {
        let c = column(arena, store, db_fqn, db, cm, expanding)?;
        columns.push(if cm.primary_key && c.multiplicity != Multiplicity::ONE {
            Column {
                name: c.name,
                r#type: c.r#type,
                multiplicity: Multiplicity::ONE,
            }
        } else {
            c
        });
    }
    RelationType::of(arena, &columns).map_err(Thrown::from_type_error)
}

fn column<'a, A: Alloc>(
    arena: &'a A,
    store: &dyn StoreLookups,
    db_fqn: &JStr,
    db: Option<&DatabaseDefinition>,
    cm: &DatabaseDefinitionViewDefinitionViewColumnMapping,
    expanding: &mut IdSet<View>,
) -> Result<Column<'a>, Thrown> {
    let name = arena.alloc_str(&cm.name.lossy());
    let expr = &cm.expression;
    if let Op::RelationalOperationColumnRef(cr) = expr {
        let src_db = cr.database_name.clone().unwrap_or_else(|| db_fqn.clone());
        if let Some(td) = store.find_table_definition(&src_db, &cr.table) {
            for col in td.columns.iter() {
                if col.name == cr.column {
                    return Ok(Column {
                        name,
                        r#type: store_compiler::column_type(&col.data_type)?,
                        multiplicity: if col.not_null || col.primary_key {
                            Multiplicity::ONE
                        } else {
                            Multiplicity::ZERO_ONE
                        },
                    });
                }
            }
        }
        if let Some(inner) = store.find_view(&src_db, &cr.table) {
            let inner = inner.clone();
            for ic in of_in(arena, store, &src_db, &inner, expanding)?.columns() {
                if cr.column.eq_str(ic.name) {
                    return Ok(Column {
                        name,
                        r#type: ic.r#type,
                        multiplicity: ic.multiplicity,
                    });
                }
            }
        }
        return Ok(any(name));
    }
    if let Op::RelationalOperationFunctionCall(fc) = expr
        && fc.args.len() == 1
    {
        let lowered = java_string::to_lower_root(&fc.name.0);
        let function = String::from_utf16(&lowered).unwrap_or_default();
        match function.as_str() {
            "count" => {
                return Ok(Column {
                    name,
                    r#type: Type::Primitive(Primitive::Integer),
                    multiplicity: Multiplicity::ONE,
                });
            }
            "average" => {
                return Ok(Column {
                    name,
                    r#type: Type::Primitive(Primitive::Float),
                    multiplicity: Multiplicity::ONE,
                });
            }
            "sum" | "max" | "min" => {
                // Pure's reducer overloads: Integer and Float keep their type; every other number (Decimal, Number)
                // is Number; max / min over a date keep the date type, over anything else the generic collection
                // reducer keeps the element type
                let arg = inferred(&fc.args[0], db, store)?;
                let numeric = matches!(arg, Type::PrecisionDecimal(_))
                    || matches!(arg, Type::Primitive(p) if p.family() == Family::Numeric);
                let out = if arg == Type::Primitive(Primitive::Integer)
                    || arg == Type::Primitive(Primitive::Float)
                    || !numeric
                {
                    arg
                } else {
                    Type::Primitive(Primitive::Number)
                };
                return Ok(Column {
                    name,
                    r#type: out,
                    multiplicity: if function == "sum" {
                        Multiplicity::ONE
                    } else {
                        Multiplicity::ZERO_ONE
                    },
                });
            }
            _ => {}
        }
    }
    Ok(Column {
        name,
        r#type: inferred(expr, db, store)?,
        multiplicity: Multiplicity::ZERO_ONE,
    })
}

/// `inferred(expr, db, store)`: the engine's inferred relational type as a Pure type; `Any` when the engine's rule has
/// none.
fn inferred(expr: &Op, db: Option<&DatabaseDefinition>, store: &dyn StoreLookups) -> Result<Type<'static>, Thrown> {
    match relational_type_inference::infer(expr, db, Some(store))? {
        None => Ok(Type::ClassType(platform::ANY)),
        Some(dt) => store_compiler::column_type(&dt),
    }
}

/// `any(name)`: `name:Any[0..1]`.
fn any(name: &str) -> Column<'_> {
    Column {
        name,
        r#type: Type::ClassType(platform::ANY),
        multiplicity: Multiplicity::ZERO_ONE,
    }
}
