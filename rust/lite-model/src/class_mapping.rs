// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ClassMapping`'s and `PropertyMapping`'s methods and convenience constructors the normalizer calls (2026-11-14, phase
//! 3's normalizer slice, step 3: the model methods, docs/rust-port/EF_CENSUS.md §8): `Relational.aggregationAwareMain`,
//! `RelationFunction`'s six-argument constructor, `RelationFunction.Col`'s plain constructor, `isEmbedded` and `bindSrc`,
//! and `PropertyMapping`'s three convenience constructors (`Embedded`'s two-argument form, which drops `~primaryKey`;
//! `Join`'s three; `JoinTerminalColumn`'s four). Each convenience constructor names Java's: the components it fills,
//! filled as Java's canonical constructor fills them (an absent list the one empty list), and its canonical
//! constructor's refusals (by class and message, as `Thrown`). `ClassMapping.Relational` has no short form, by its own
//! rule (a defaulted `propertyTargetSets` dropped routing at rebuild sites): a rebuild names every component.

use std::sync::Arc;

use lite_front::jtypes::JStr;
use lite_front::records::{ValueSpecification, Variable};

use crate::error::{Thrown, illegal_argument};
use crate::linked::empty;
use crate::records::{
    ClassMappingRelationFunction, ClassMappingRelationFunctionCol, ClassMappingRelational, JoinChainElement,
    PropertyMapping, PropertyMappingEmbedded, PropertyMappingJoin, PropertyMappingJoinTerminalColumn,
    RelationalOperation,
};

impl ClassMappingRelational {
    /// `aggregationAwareMain()`: whether this set is an AggregationAware mapping's main set (it carries the node's
    /// aggregate views).
    pub fn aggregation_aware_main(&self) -> bool {
        self.aggregation.is_some()
    }
}

impl ClassMappingRelationFunction {
    /// `new RelationFunction(className, setId, extendsSetId, root, funcRef, columns)`: a relation mapping over a named
    /// `~func`, with no inline source and no `~primaryKey` of its own. The canonical constructor's one refusal (exactly
    /// one of `funcRef`/`inlineSource`) cannot fire: the reference is given and the inline source is absent.
    pub fn new(
        class_name: JStr,
        set_id: Option<JStr>,
        extends_set_id: Option<JStr>,
        root: bool,
        func_ref: JStr,
        columns: Arc<[Arc<ClassMappingRelationFunctionCol>]>,
    ) -> ClassMappingRelationFunction {
        ClassMappingRelationFunction {
            class_name,
            set_id,
            extends_set_id,
            root,
            func_ref: Some(func_ref),
            inline_source: None,
            columns,
            primary_key: empty(),
        }
    }
}

impl ClassMappingRelationFunctionCol {
    /// `new Col(property, column, local)`: a plain (non-enum) column binding -- no enumeration mapping, no embedded
    /// block, no inline set, no row expression.
    pub fn new(property: JStr, column: JStr, local: bool) -> ClassMappingRelationFunctionCol {
        ClassMappingRelationFunctionCol {
            property,
            column: Some(column),
            local,
            enum_mapping_id: None,
            embedded: empty(),
            inline_set_id: None,
            expr: None,
        }
    }

    /// `isEmbedded()`: an embedded block (`prop ( sub: COL, ... )`) -- the property is class-typed and its
    /// sub-bindings share this row.
    pub fn is_embedded(&self) -> bool {
        !self.embedded.is_empty()
    }

    /// `Col.bindSrc(v, row)`: every `$src` in a row-expression binding replaced by the row variable -- `row` itself
    /// for a `$src`, `v` itself (its very `Arc`) when it holds none, and otherwise the node rebuilt through
    /// `mapChildren` (so a rebuilt property loses its position, a rebuilt lambda its own, as Java's rebuilds do). A
    /// replaced `$src` always counts as a change here; in Java it does unless `row` is that very `$src` object (the
    /// one place the two can differ: a caller that hands the tree's own `$src` as the row).
    pub fn bind_src(v: &ValueSpecification, row: &ValueSpecification) -> ValueSpecification {
        bound(v, row).unwrap_or_else(|| v.clone())
    }
}

/// `bindSrc`'s recursion: `None` for `v` itself.
fn bound(v: &ValueSpecification, row: &ValueSpecification) -> Option<ValueSpecification> {
    if is_src(v) {
        return Some(row.clone());
    }
    // a lambda whose parameter is named src rebinds it (Pure's innermost-first scoping; since 2026-11-15, the
    // normalizer's second batch, item 1, RR:131)
    if let ValueSpecification::LambdaFunction(lf) = v
        && lf.parameters.iter().any(|p| is_named_src(p))
    {
        return None;
    }
    v.map_children(|c| bound(c, row))
}

fn is_src(v: &ValueSpecification) -> bool {
    matches!(v, ValueSpecification::Variable(var) if is_named_src(var))
}

fn is_named_src(var: &Variable) -> bool {
    var.name == "src"
}

impl PropertyMappingEmbedded {
    /// `new Embedded(propertyName, propertyMappings)`: an embedded block with no `~primaryKey` (the two-argument form
    /// drops it: the primary key is the one empty list).
    pub fn new(property_name: JStr, property_mappings: Arc<[PropertyMapping]>) -> PropertyMappingEmbedded {
        PropertyMappingEmbedded {
            property_name,
            property_mappings,
            primary_key: empty(),
        }
    }
}

impl PropertyMappingJoin {
    /// `new Join(propertyName, database, joins)`: a join property mapping with no target set; at least one hop, as
    /// the canonical constructor requires (its `IllegalArgumentException`, by message).
    pub fn new(
        property_name: JStr,
        database: JStr,
        joins: Arc<[Arc<JoinChainElement>]>,
    ) -> Result<PropertyMappingJoin, Thrown> {
        if joins.is_empty() {
            return Err(illegal_argument(JStr::from(
                "Join property mapping must have at least one join hop",
            )));
        }
        Ok(PropertyMappingJoin {
            property_name,
            database,
            joins,
            target_set_id: None,
        })
    }
}

impl PropertyMappingJoinTerminalColumn {
    /// `new JoinTerminalColumn(propertyName, database, joins, terminalColumn)`: no enumeration mapping, not
    /// enum-mapped; at least one hop, as the canonical constructor requires (its `IllegalArgumentException`).
    pub fn new(
        property_name: JStr,
        database: JStr,
        joins: Arc<[Arc<JoinChainElement>]>,
        terminal_column: RelationalOperation,
    ) -> Result<PropertyMappingJoinTerminalColumn, Thrown> {
        if joins.is_empty() {
            return Err(illegal_argument(JStr::from(
                "Join-with-terminal property mapping must have at least one join hop",
            )));
        }
        Ok(PropertyMappingJoinTerminalColumn {
            property_name,
            database,
            joins,
            terminal_column,
            enum_mapping_id: None,
            enum_mapped: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lite_front::codec;
    use lite_front::doors::Dialect;

    use crate::records::{
        ClassMapping, ClassMappingAggregationAware, PropertyMappingColumn, RelationalOperationColumnRef,
    };

    fn parse(text: &str) -> ValueSpecification {
        let units: Vec<u16> = text.encode_utf16().collect();
        lite_front::spec_parser::parse(&units, Dialect::LegendLite).expect("the expression parses")
    }

    fn json<T: codec::Codec + ?Sized>(v: &T) -> String {
        codec::write(v, false)
    }

    fn row() -> ValueSpecification {
        ValueSpecification::Variable(Arc::new(Variable {
            name: JStr::from("row"),
            r#type: None,
            multiplicity: None,
            pos: None,
        }))
    }

    #[test]
    fn bind_src_replaces_src_and_rebuilds_its_parents() {
        let v = parse("$src.COL");
        let ValueSpecification::AppliedProperty(before) = &v else {
            panic!("a property")
        };
        assert!(before.pos.is_some());
        let bound = ClassMappingRelationFunctionCol::bind_src(&v, &row());
        let ValueSpecification::AppliedProperty(after) = &bound else {
            panic!("a property")
        };
        assert_eq!(after.receiver, row());
        assert_eq!(after.property, before.property);
        assert_eq!(
            after.pos, None,
            "the rebuilt property drops its position, as Java's rebuild does"
        );
    }

    #[test]
    fn bind_src_leaves_a_lambda_that_rebinds_src() {
        // Pure's innermost-first scoping: the lambda's `$src` is its parameter (the normalizer's second batch, item 1,
        // RR:131, 2026-11-15; legend-engine 4.145.0 plans `$src.N->map(src | $src + 1)` as `N + 1`)
        let v = parse("$src.N->map(src | $src + 1)");
        let bound = ClassMappingRelationFunctionCol::bind_src(&v, &row());
        let ValueSpecification::AppliedFunction(af) = &bound else {
            panic!("a call")
        };
        let ValueSpecification::AppliedProperty(n) = &af.parameters[0] else {
            panic!("the receiver")
        };
        assert_eq!(n.receiver, row());
        let ValueSpecification::AppliedFunction(before) = &v else {
            panic!("a call")
        };
        assert_eq!(
            json(&af.parameters[1]),
            json(&before.parameters[1]),
            "the lambda untouched"
        );
    }

    #[test]
    fn bind_src_answers_a_bare_src_with_the_row_and_a_src_free_tree_unchanged() {
        assert_eq!(ClassMappingRelationFunctionCol::bind_src(&parse("$src"), &row()), row());
        let v = parse("[$x.COL, 'lit']");
        let same = ClassMappingRelationFunctionCol::bind_src(&v, &row());
        assert_eq!(
            json(&same),
            json(&v),
            "nothing to bind: the very tree, positions and all"
        );
        let ValueSpecification::PureCollection(c) = parse("[$src.A, $x.B]") else {
            panic!("a collection")
        };
        let ValueSpecification::PureCollection(b) =
            ClassMappingRelationFunctionCol::bind_src(&ValueSpecification::PureCollection(c.clone()), &row())
        else {
            panic!("a collection")
        };
        assert_eq!(b.pos, c.pos, "a rebuilt collection keeps its position");
        assert_eq!(
            json(&b.values[1]),
            json(&c.values[1]),
            "the untouched read is the very one"
        );
    }

    #[test]
    fn a_column_binding_is_plain_and_embedded_only_with_sub_bindings() {
        let plain = ClassMappingRelationFunctionCol::new(JStr::from("name"), JStr::from("NAME"), false);
        assert!(!plain.is_embedded());
        assert_eq!(
            json(&plain),
            "{\"@\":\"model.ClassMapping$RelationFunction$Col\",\"property\":\"name\",\"column\":\"NAME\",\
             \"local\":false,\"enumMappingId\":null,\"embedded\":[],\"inlineSetId\":null,\"expr\":null}"
        );
        let block = ClassMappingRelationFunctionCol {
            embedded: Arc::from(vec![Arc::new(plain.clone())]),
            ..plain.clone()
        };
        assert!(block.is_embedded());
        let rf = ClassMappingRelationFunction::new(
            JStr::from("a::P"),
            Some(JStr::from("p")),
            None,
            true,
            JStr::from("<relational>"),
            Arc::from(vec![Arc::new(plain)]),
        );
        assert_eq!(rf.func_ref, Some(JStr::from("<relational>")));
        assert_eq!(rf.inline_source, None);
        assert!(rf.primary_key.is_empty());
    }

    #[test]
    fn the_main_set_of_an_aggregation_aware_mapping_carries_its_views() {
        let r = ClassMappingRelational {
            class_name: JStr::from("a::P"),
            set_id: None,
            extends_set_id: None,
            root: true,
            main_table: None,
            filter: None,
            distinct: false,
            group_by: empty(),
            primary_key: empty(),
            property_mappings: empty(),
            source_url: None,
            property_target_sets: empty(),
            aggregation: None,
        };
        assert!(!r.aggregation_aware_main());
        let main = ClassMappingRelational {
            aggregation: Some(Arc::new(ClassMappingAggregationAware { views: empty() })),
            ..r
        };
        assert!(main.aggregation_aware_main());
        let _ = ClassMapping::ClassMappingRelational(Arc::new(main));
    }

    #[test]
    fn the_property_mappings_short_forms_fill_as_java_does() {
        let col = PropertyMapping::PropertyMappingColumn(Arc::new(PropertyMappingColumn {
            property_name: JStr::from("a"),
            database: JStr::from("db::D"),
            table: JStr::from("T"),
            column: JStr::from("A"),
        }));
        let e = PropertyMappingEmbedded::new(JStr::from("firm"), Arc::from(vec![col]));
        assert!(e.primary_key.is_empty(), "the two-argument form drops ~primaryKey");
        assert!(json(&e).ends_with(",\"primaryKey\":[]}"), "{}", json(&e));

        let hop = Arc::new(JoinChainElement {
            join_name: JStr::from("J"),
            join_type: None,
            database_name: None,
            include_self: false,
        });
        let j = PropertyMappingJoin::new(JStr::from("firm"), JStr::from("db::D"), Arc::from(vec![hop.clone()]))
            .expect("one hop");
        assert_eq!(j.target_set_id, None);
        assert_eq!(
            PropertyMappingJoin::new(JStr::from("firm"), JStr::from("db::D"), empty()),
            Err(illegal_argument(JStr::from(
                "Join property mapping must have at least one join hop"
            )))
        );
        let terminal = RelationalOperation::RelationalOperationColumnRef(Arc::new(RelationalOperationColumnRef {
            database_name: None,
            table: JStr::from("F"),
            column: JStr::from("NAME"),
        }));
        let t = PropertyMappingJoinTerminalColumn::new(
            JStr::from("firmName"),
            JStr::from("db::D"),
            Arc::from(vec![hop]),
            terminal.clone(),
        )
        .expect("one hop");
        assert_eq!((t.enum_mapping_id, t.enum_mapped), (None, false));
        assert_eq!(
            PropertyMappingJoinTerminalColumn::new(JStr::from("firmName"), JStr::from("db::D"), empty(), terminal),
            Err(illegal_argument(JStr::from(
                "Join-with-terminal property mapping must have at least one join hop"
            )))
        );
    }
}
