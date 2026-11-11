// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! THE TYPE FAMILY'S DIFFERENTIAL (docs/rust-port/PHASE3.md, slice 1a, 2026-11-11): every line of Java's answers,
//! frozen in rust/oracles/types/type-facts.tsv by rust/oracles/types/TypeOracle.java (its header says what a line is),
//! asked of lite-types and held to Java's answer byte for byte -- a refusal by its exception's class and message.
//! Every sample type is built twice, in a query's arena and in the model's, and written back to the oracle's text.
//! The oracle's header counts its lines, op by op; every one must be asked, and an op this test does not know fails
//! it, so a truncated or grown oracle cannot pass. Bazel names the oracle (`LITE_TYPE_ORACLE`); cargo reads it from
//! the tree.

use std::collections::BTreeMap;

use lite_base::{Alloc, ModelArena, QueryArena};
use lite_json::Node;
use lite_sql::{Scalar, SqlType, StructField, TaggedSlot};
use lite_types::{
    Bounded, Column, Family, FunctionType, GenericType, Multiplicity, Op, PRECISE_PRIMITIVES, Param, PrecisionDecimal,
    Primitive, RelationType, SchemaAlgebra, Type, TypeError, Var, platform,
};

/// The oracle's size when it was frozen: fewer lines asked is a truncated oracle.
const FLOOR: usize = 26_429;

// ---- the oracle's text form, written

fn s(out: &mut String, text: &str) {
    out.push('"');
    lite_json::escape_to(out, text, false);
    out.push('"');
}

fn text(t: &str) -> String {
    let mut out = String::new();
    s(&mut out, t);
    out
}

fn mult(out: &mut String, m: Multiplicity<'_>) {
    match m {
        Multiplicity::Bounded(b) => match b.upper() {
            Some(u) => out.push_str(&format!("[{},{u}]", b.lower())),
            None => out.push_str(&format!("[{},null]", b.lower())),
        },
        Multiplicity::Var(v) => s(out, v.name()),
    }
}

fn list<T: Copy>(out: &mut String, xs: &[T], mut each: impl FnMut(&mut String, T)) {
    out.push('[');
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        each(out, *x);
    }
    out.push(']');
}

fn param(out: &mut String, p: Param<'_>) {
    out.push('[');
    ty(out, p.r#type);
    out.push(',');
    mult(out, p.multiplicity);
    out.push(']');
}

fn column(out: &mut String, c: Column<'_>) {
    out.push('[');
    s(out, c.name);
    out.push(',');
    ty(out, c.r#type);
    out.push(',');
    mult(out, c.multiplicity);
    out.push(']');
}

fn ty(out: &mut String, t: Type<'_>) {
    match t {
        Type::Primitive(p) => {
            out.push_str("[\"P\",");
            s(out, p.name());
            out.push(']');
        }
        Type::PrecisionDecimal(d) => out.push_str(&format!("[\"D\",{},{}]", d.precision(), d.scale())),
        Type::ClassType(f) | Type::EnumType(f) | Type::TypeVar(f) => {
            let tag = match t {
                Type::ClassType(_) => "C",
                Type::EnumType(_) => "E",
                _ => "V",
            };
            out.push_str(&format!("[\"{tag}\","));
            s(out, f);
            out.push(']');
        }
        Type::GenericType(g) => {
            out.push_str("[\"G\",");
            s(out, g.raw_fqn);
            out.push(',');
            list(out, g.arguments, ty);
            out.push(',');
            list(out, g.mult_arguments, mult);
            out.push(']');
        }
        Type::FunctionType(f) => {
            out.push_str("[\"F\",");
            list(out, f.params, param);
            out.push(',');
            param(out, f.result);
            out.push(']');
        }
        Type::RelationType(r) => {
            out.push_str("[\"R\",");
            list(out, r.columns(), column);
            out.push(',');
            list(out, r.dynamic_columns(), column);
            out.push(']');
        }
        Type::SchemaAlgebra(a) => {
            out.push_str("[\"S\",");
            ty(out, a.left);
            out.push(',');
            s(out, a.op.name());
            out.push(',');
            ty(out, a.right);
            out.push(']');
        }
    }
}

fn type_text(t: Type<'_>) -> String {
    let mut out = String::new();
    ty(&mut out, t);
    out
}

fn mult_text(m: Multiplicity<'_>) -> String {
    let mut out = String::new();
    mult(&mut out, m);
    out
}

fn column_text(c: Column<'_>) -> String {
    let mut out = String::new();
    column(&mut out, c);
    out
}

fn decimal_text(d: PrecisionDecimal) -> String {
    type_text(Type::PrecisionDecimal(d))
}

fn bool_text(b: bool) -> String {
    b.to_string()
}

fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> String) -> String {
    v.map_or_else(|| "null".to_owned(), f)
}

fn refused(e: &TypeError) -> String {
    let mut out = String::from("{\"throws\":");
    s(&mut out, e.thrown.simple_name());
    out.push_str(",\"message\":");
    s(&mut out, &e.message);
    out.push('}');
    out
}

fn answer<T>(r: Result<T, TypeError>, f: impl FnOnce(T) -> String) -> String {
    match r {
        Ok(v) => f(v),
        Err(e) => refused(&e),
    }
}

// ---- the oracle's text form, read into either arena

fn json(field: &str) -> Node {
    lite_json::parse(field).unwrap_or_else(|e| panic!("{field}: {e}"))
}

fn arr(n: &Node) -> &[Node] {
    n.as_arr().unwrap_or_else(|e| panic!("{n:?}: {e}"))
}

fn st(n: &Node) -> &str {
    n.as_str().unwrap_or_else(|e| panic!("{n:?}: {e}"))
}

fn int(n: &Node) -> i32 {
    match n {
        Node::Num(v) => i32::try_from(v.as_i64().expect("an integer")).expect("an int"),
        _ => panic!("{n:?}: not a number"),
    }
}

fn read_mult<'a, A: Alloc>(a: &'a A, n: &Node) -> Multiplicity<'a> {
    match n {
        Node::Str(name) => Multiplicity::Var(Var::new(a.alloc_str(name)).expect("a variable")),
        _ => {
            let b = arr(n);
            let upper = match &b[1] {
                Node::Null => None,
                u => Some(int(u)),
            };
            Multiplicity::Bounded(Bounded::new(int(&b[0]), upper).expect("bounds"))
        }
    }
}

fn read_columns<'a, A: Alloc>(a: &'a A, n: &Node) -> Vec<Column<'a>> {
    arr(n)
        .iter()
        .map(|c| {
            let c = arr(c);
            Column {
                name: a.alloc_str(st(&c[0])),
                r#type: read_type(a, &c[1]),
                multiplicity: read_mult(a, &c[2]),
            }
        })
        .collect()
}

fn read_param<'a, A: Alloc>(a: &'a A, n: &Node) -> Param<'a> {
    let p = arr(n);
    Param {
        r#type: read_type(a, &p[0]),
        multiplicity: read_mult(a, &p[1]),
    }
}

fn read_type<'a, A: Alloc>(a: &'a A, n: &Node) -> Type<'a> {
    let t = arr(n);
    match st(&t[0]) {
        "P" => Type::Primitive(Primitive::from_name(st(&t[1])).expect("a primitive")),
        "D" => Type::PrecisionDecimal(PrecisionDecimal::new(int(&t[1]), int(&t[2])).expect("a decimal")),
        "C" => Type::ClassType(a.alloc_str(st(&t[1]))),
        "E" => Type::EnumType(a.alloc_str(st(&t[1]))),
        "V" => Type::TypeVar(a.alloc_str(st(&t[1]))),
        "G" => {
            let args: Vec<Type<'a>> = arr(&t[2]).iter().map(|x| read_type(a, x)).collect();
            let mults: Vec<Multiplicity<'a>> = arr(&t[3]).iter().map(|x| read_mult(a, x)).collect();
            Type::GenericType(GenericType::new(a, a.alloc_str(st(&t[1])), &args, &mults))
        }
        "F" => {
            let params: Vec<Param<'a>> = arr(&t[1]).iter().map(|x| read_param(a, x)).collect();
            Type::FunctionType(FunctionType::new(a, &params, read_param(a, &t[2])))
        }
        "R" => Type::RelationType(
            RelationType::new(a, &read_columns(a, &t[1]), &read_columns(a, &t[2])).expect("a relation type"),
        ),
        "S" => {
            let op = Op::ALL.into_iter().find(|o| o.name() == st(&t[2])).expect("an op");
            Type::SchemaAlgebra(SchemaAlgebra::new(a, read_type(a, &t[1]), op, read_type(a, &t[3])))
        }
        other => panic!("no type tag {other}"),
    }
}

fn sql_type(spec: &str) -> SqlType {
    let scalar = |name: &str| SqlType::Scalar(Scalar::from_name(name).expect("a scalar"));
    let (kind, rest) = spec.split_once(':').expect("kind:rest");
    match kind {
        "scalar" => scalar(rest),
        "decimal" => {
            let (p, sc) = rest.split_once(',').expect("p,s");
            SqlType::Decimal {
                precision: p.parse().unwrap(),
                scale: sc.parse().unwrap(),
            }
        }
        "array" => SqlType::Array {
            element: Box::new(scalar(rest)),
        },
        "map" => {
            let (k, v) = rest.split_once(',').expect("k,v");
            SqlType::Map {
                key: Box::new(scalar(k)),
                value: Box::new(scalar(v)),
            }
        }
        "struct" => {
            let (name, t) = rest.split_once(':').expect("name:type");
            SqlType::Struct {
                fields: vec![StructField {
                    name: name.to_owned(),
                    r#type: scalar(t),
                }],
            }
        }
        "tagged" => {
            let (k, t) = rest.split_once(':').expect("kind:type");
            SqlType::Tagged {
                slots: vec![TaggedSlot {
                    kind: k.to_owned(),
                    r#type: scalar(t),
                }],
            }
        }
        other => panic!("no SQL type kind {other}"),
    }
}

fn constant(name: &str) -> String {
    if let Some(field) = name.strip_prefix("PlatformTypes.") {
        if let Some((_, v)) = platform::NAMED.iter().find(|(n, _)| *n == field) {
            return text(v);
        }
        let (_, set) = platform::NAMED_SETS
            .iter()
            .find(|(n, _)| *n == field)
            .unwrap_or_else(|| panic!("no {name}"));
        let mut sorted = set.to_vec();
        sorted.sort_unstable();
        let mut out = String::new();
        list(&mut out, &sorted, s);
        return out;
    }
    match name {
        "Type.Primitive.PRECISE_PRIMITIVES" => text(PRECISE_PRIMITIVES),
        "Type.RelationType.PIVOT_SEPARATOR" => text(lite_types::PIVOT_SEPARATOR),
        "Type.RelationType.LATE_BOUND_WILDCARD" => text(lite_types::LATE_BOUND_WILDCARD),
        "Type.PrecisionDecimal.DEFAULT_DECIMAL" => decimal_text(PrecisionDecimal::DEFAULT_DECIMAL),
        "Type.PrecisionDecimal.MAX_PRECISION" => PrecisionDecimal::MAX_PRECISION.to_string(),
        "Type.PrecisionDecimal.MIN_ADJUSTED_SCALE" => PrecisionDecimal::MIN_ADJUSTED_SCALE.to_string(),
        "Multiplicity.Bounded.ONE" => mult_text(Bounded::ONE.into()),
        "Multiplicity.Bounded.ZERO_ONE" => mult_text(Bounded::ZERO_ONE.into()),
        "Multiplicity.Bounded.ONE_MANY" => mult_text(Bounded::ONE_MANY.into()),
        "Multiplicity.Bounded.ZERO_MANY" => mult_text(Bounded::ZERO_MANY.into()),
        other => panic!("no constant {other}"),
    }
}

fn index(field: &str) -> usize {
    field.parse().unwrap_or_else(|_| panic!("{field}: not an index"))
}

struct Samples<'a> {
    types: Vec<Type<'a>>,
    mults: Vec<Multiplicity<'a>>,
    decimals: Vec<PrecisionDecimal>,
}

/// Rust's answer to one line's question: the line's op and arguments (its answer field left off).
fn ask<'a>(cx: &'a QueryArena, k: &Samples<'a>, op: &str, args: &[&str]) -> String {
    let t = || k.types[index(args[0])];
    let m = || k.mults[index(args[0])];
    let d = || k.decimals[index(args[0])];
    let arg_str = |i: usize| -> String { st(&json(args[i])).to_owned() };
    match op {
        "op.symbol" => {
            let name = arg_str(0);
            text(Op::ALL.into_iter().find(|o| o.name() == name).expect("an op").symbol())
        }
        "family" => text(Family::ALL[index(args[0])].name()),
        "primitive" => text(Primitive::ALL[index(args[0])].name()),
        "primitive.byFqn" => {
            let (fqn, p) = Primitive::by_fqn()[index(args[0])];
            format!("[{},{}]", text(fqn), text(p.name()))
        }
        "primitive.ofType" => match t() {
            Type::Primitive(p) => text(p.name()),
            other => panic!("{other:?}: not a primitive"),
        },
        _ if op.starts_with("primitive.") => {
            let p = Primitive::from_name(&arg_str(0)).expect("a primitive");
            match op {
                "primitive.qualifiedName" => text(p.qualified_name()),
                "primitive.typeName" => text(p.type_name()),
                "primitive.family" => text(p.family().name()),
                "primitive.isNumeric" => bool_text(p.is_numeric()),
                "primitive.isTemporal" => bool_text(p.is_temporal()),
                _ => panic!("no op {op}"),
            }
        }
        _ if op.starts_with("string.") => string_op(cx, op, &arg_str(0), args),
        "type.equals" => k
            .types
            .iter()
            .map(|u| if *u == t() { '1' } else { '0' })
            .collect::<String>(),
        "type.relationValued" => bool_text(Type::relation_valued(lite_types::ExprType {
            r#type: t(),
            multiplicity: k.mults[index(args[1])],
        })),
        _ if op.starts_with("type.") => type_op(op, t()),
        "decimal.basePrimitive" => match t() {
            Type::PrecisionDecimal(pd) => type_text(Type::Primitive(pd.base_primitive())),
            other => panic!("{other:?}: not a decimal"),
        },
        "generic.isClassMetaclass" => match t() {
            Type::GenericType(g) => bool_text(platform::is_class_metaclass(g)),
            other => panic!("{other:?}: not generic"),
        },
        "function.lambdaType" | "function.concreteFunctionDefinitionType" => match t() {
            Type::FunctionType(f) if op == "function.lambdaType" => type_text(platform::lambda_type(cx, f)),
            Type::FunctionType(f) => type_text(platform::concrete_function_definition_type(cx, f)),
            other => panic!("{other:?}: not a function type"),
        },
        _ if op.starts_with("relation.") => {
            let Type::RelationType(r) = t() else {
                panic!("not a relation type")
            };
            match op {
                "relation.relation" => type_text(Type::relation(cx, r)),
                "relation.isLateBound" => bool_text(r.is_late_bound()),
                "relation.isErasedRow" => bool_text(r.is_erased_row()),
                "relation.pivotColumnType" => answer(r.pivot_column_type(&arg_str(1)), |found| opt(found, type_text)),
                _ => panic!("no op {op}"),
            }
        }
        "mult.text" => text(&m().text()),
        "mult.isMany" => bool_text(m().is_many()),
        "mult.requireBounded" => answer(m().require_bounded("the oracle"), |b| mult_text(b.into())),
        "mult.isUnbounded" | "mult.isToOne" => match m() {
            Multiplicity::Bounded(b) if op == "mult.isUnbounded" => bool_text(b.is_unbounded()),
            Multiplicity::Bounded(b) => bool_text(b.is_to_one()),
            other => panic!("{other:?}: not bounded"),
        },
        "mult.union" => answer(Multiplicity::union(m(), k.mults[index(args[1])]), mult_text),
        "mult.product" => answer(Multiplicity::product(m(), k.mults[index(args[1])]), mult_text),
        "decimal.typeName" => text(&d().type_name()),
        "decimal.plus" | "decimal.minus" | "decimal.times" | "decimal.dividedBy" => {
            let e = k.decimals[index(args[1])];
            let r = match op {
                "decimal.plus" => d().plus(e),
                "decimal.minus" => d().minus(e),
                "decimal.times" => d().times(e),
                _ => d().divided_by(e),
            };
            answer(r, decimal_text)
        }
        "new.decimal" => {
            let ps = json(args[0]);
            answer(
                PrecisionDecimal::new(int(&arr(&ps)[0]), int(&arr(&ps)[1])),
                decimal_text,
            )
        }
        "new.bounded" => {
            let lu = json(args[0]);
            let upper = match &arr(&lu)[1] {
                Node::Null => None,
                u => Some(int(u)),
            };
            answer(Bounded::new(int(&arr(&lu)[0]), upper), |b| mult_text(b.into()))
        }
        "new.var" => {
            let name = cx.alloc_str(&arg_str(0));
            answer(Var::new(name), |v| mult_text(Multiplicity::Var(v)))
        }
        "new.relation" => {
            let cd = json(args[0]);
            let (cols, dyns) = (read_columns(cx, &arr(&cd)[0]), read_columns(cx, &arr(&cd)[1]));
            answer(RelationType::new(cx, &cols, &dyns), |r| {
                type_text(Type::RelationType(r))
            })
        }
        "ofArgument" => {
            let spelling = cx.alloc_str(&arg_str(0));
            answer(Multiplicity::of_argument(spelling), mult_text)
        }
        "kindOfSqlType" => opt(Type::kind_of_sql_type(&sql_type(&arg_str(0))), type_text),
        _ => panic!("no op {op}: the oracle asks something this test does not know"),
    }
}

fn string_op(cx: &QueryArena, op: &str, s: &str, args: &[&str]) -> String {
    match op {
        "string.isTypeClassifier" => bool_text(platform::is_type_classifier(s)),
        "string.isStoreNavFn" => bool_text(platform::is_store_nav_fn(s)),
        "string.isRelationalConnectionClass" => bool_text(platform::is_relational_connection_class(s)),
        "string.relationalConnectionSimpleName" => opt(platform::relational_connection_simple_name(s), text),
        "string.isModelChainConnection" => bool_text(platform::is_model_chain_connection(s)),
        "string.isJsonModelConnection" => bool_text(platform::is_json_model_connection(s)),
        "string.isPlus" => bool_text(platform::is_plus(s)),
        "string.isVerdictFunction" => bool_text(platform::is_verdict_function(s)),
        "string.isPostProcessorConfigProperty" => bool_text(platform::is_post_processor_config_property(s)),
        "string.isProfile" => bool_text(platform::is_profile(s, st(&json(args[1])))),
        "string.typeValuePath" => text(platform::type_value_path_of(s)),
        "string.findByFqn" => opt(Primitive::find_by_fqn(s), |p| type_text(Type::Primitive(p))),
        "string.typeVariables" => opt(Primitive::type_variables(s), |vs| {
            let mut out = String::new();
            list(&mut out, vs, |out, v| {
                out.push_str(&format!("[{},{}]", text(v.name), text(v.r#type.name())))
            });
            out
        }),
        "string.presentPivotName" => text(&RelationType::present_pivot_name(s)),
        "string.trustedColumn" => column_text(RelationType::trusted_column(cx.alloc_str(s))),
        "string.erasedRow" => type_text(Type::RelationType(RelationType::erased_row(cx, cx.alloc_str(s)))),
        _ => panic!("no op {op}"),
    }
}

fn type_op(op: &str, t: Type<'_>) -> String {
    match op {
        "type.typeName" => text(&t.type_name()),
        "type.isRelation" => bool_text(t.is_relation()),
        "type.classFqn" => opt(t.class_fqn(), text),
        "type.asClassType" => opt(t.as_class_type(), type_text),
        "type.relationSchema" => opt(t.relation_schema(), |r| type_text(Type::RelationType(r))),
        "type.schemaView" => opt(t.schema_view(), |r| type_text(Type::RelationType(r))),
        "type.requireRelationSchema" => answer(t.require_relation_schema(), |r| type_text(Type::RelationType(r))),
        "type.wireDecided" => bool_text(t.wire_decided()),
        "type.isAny" => bool_text(platform::is_any(t)),
        "type.isNil" => bool_text(platform::is_nil(t)),
        "type.isVariant" => bool_text(platform::is_variant(t)),
        "type.isJsonElement" => bool_text(platform::is_json_element(t)),
        "type.isListCarrier" => bool_text(platform::is_list_carrier(t)),
        "type.isPairCarrier" => bool_text(platform::is_pair_carrier(t)),
        "type.isMapCarrier" => bool_text(platform::is_map_carrier(t)),
        "type.isValueCarrier" => bool_text(platform::is_value_carrier(t)),
        "type.isFunctionCarrier" => bool_text(platform::is_function_carrier(t)),
        "type.isTdsShaped" => bool_text(platform::is_tds_shaped(t)),
        "type.isTdsType" => bool_text(platform::is_tds_type(t)),
        "type.printsByOwnToString" => bool_text(platform::prints_by_own_to_string(t)),
        "type.typeValuePath" => text(&platform::type_value_path(t)),
        "type.functionTypeOf" => opt(platform::function_type_of(t), |f| type_text(Type::FunctionType(f))),
        _ => panic!("no op {op}"),
    }
}

fn oracle_text() -> String {
    let path = std::env::var("LITE_TYPE_ORACLE")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../oracles/types/type-facts.tsv").to_owned());
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn every_answer_of_the_type_family_is_javas() {
    let oracle = oracle_text();
    let mut header_total = None;
    let mut header_ops: BTreeMap<String, usize> = BTreeMap::new();
    let mut lines: Vec<Vec<&str>> = Vec::new();
    for line in oracle.lines() {
        if let Some(count) = line.strip_prefix("#\t") {
            let (op, n) = count.split_once('\t').expect("op\tcount");
            header_ops.insert(op.to_owned(), n.parse().expect("a count"));
        } else if let Some(rest) = line.strip_prefix("# ")
            && let Some((n, _)) = rest.split_once(" lines, ")
        {
            header_total = Some(n.parse::<usize>().expect("the line count"));
        } else if !line.starts_with('#') {
            lines.push(line.split('\t').collect());
        }
    }
    assert_eq!(
        header_total,
        Some(lines.len()),
        "the oracle's lines are not the ones its header counts"
    );
    assert!(
        lines.len() >= FLOOR,
        "{} lines, under the floor {FLOOR}: a truncated oracle",
        lines.len()
    );

    let cx = QueryArena::new();
    let model = ModelArena::new();
    let mut samples = Samples {
        types: Vec::new(),
        mults: Vec::new(),
        decimals: Vec::new(),
    };
    let mut asked: BTreeMap<String, usize> = BTreeMap::new();
    let mut wrong: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for fields in &lines {
        let (op, rest) = (fields[0], &fields[1..]);
        *asked.entry(op.to_owned()).or_default() += 1;
        let (args, expected) = rest.split_at(rest.len() - 1);
        let expected = expected[0];
        let actual = match op {
            // the samples: each read into both arenas, the two equal, and written back to Java's text
            "type" => {
                let node = json(expected);
                let built = read_type(&cx, &node);
                assert_eq!(built, read_type(&model, &node), "the two arenas' types differ");
                assert_eq!(index(args[0]), samples.types.len());
                samples.types.push(built);
                type_text(built)
            }
            "mult" => {
                let built = read_mult(&cx, &json(expected));
                assert_eq!(index(args[0]), samples.mults.len());
                samples.mults.push(built);
                mult_text(built)
            }
            "decimal" => {
                let Type::PrecisionDecimal(d) = read_type(&cx, &json(expected)) else {
                    panic!("a decimal")
                };
                assert_eq!(index(args[0]), samples.decimals.len());
                samples.decimals.push(d);
                decimal_text(d)
            }
            "const" => constant(st(&json(args[0]))),
            _ => ask(&cx, &samples, op, args),
        };
        // a type.equals answer is a JSON string of bits
        let actual = if op == "type.equals" { text(&actual) } else { actual };
        if actual != expected {
            wrong
                .entry(op.to_owned())
                .or_default()
                .push(format!("{}: Java {expected}, Rust {actual}", args.join(" ")));
        }
    }
    assert_eq!(
        asked, header_ops,
        "the ops asked are not the ones the oracle's header counts"
    );
    let report: Vec<String> = wrong
        .iter()
        .map(|(op, cases)| format!("{op}: {} of {} differ, first {}", cases.len(), asked[op], cases[0]))
        .collect();
    assert!(
        report.is_empty(),
        "{} lines differ:\n{}",
        wrong.values().map(Vec::len).sum::<usize>(),
        report.join("\n")
    );
    println!(
        "{} lines over {} ops, every one Java's answer",
        lines.len(),
        asked.len()
    );
}
