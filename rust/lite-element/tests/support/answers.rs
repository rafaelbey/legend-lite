// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! One ask's canonical text, as core's `frontoracle/ElementAnswers.java` writes it (its `form`, `run`, `answer`,
//! `boot`; the grammar is its header, the code the spec where they differ: F's ` same`/` apart`, the per-ask X line's
//! `T|F` column, the `D@` lines). D (`resolve_alongside` beside the boot layer), `normalize_with_system`, then
//! `Model::from` with the boot's checked layer less the graph's adopted ids and the platform's registrations; then
//! every question about the graph's own elements, each lazy cell forced, a refusing cell `R TAB class TAB first line`
//! in its place. A `Thrown` in a cell is its refusal; a panic is no refusal: it escapes the cell to the ask's catch (the
//! harness's), as a bug must.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use lite_base::{ModelArena, QueryArena, same};
use lite_element::{Model, Thrown, TypedFunction};
use lite_front::codec::{self, Codec, Writer};
use lite_front::doors::Dialect;
use lite_model::linked::{self, LinkedMap};
use lite_model::protocol as p;
use lite_model::{
    ClassDefinition, Function, FunctionId, JStr, KeyThread, MappingDefinition, PackageableElement, ParsedModel,
    PoisonKey,
};
use lite_names::resolver;
use lite_platform::{Implementation, Registrations};

use super::boot::{Layer, boot_checked, boot_fqns, normalize_with_system};
use super::registrations::platform_registrations;
use super::views;

/// The registrations every model is handed (ElementAnswers' `REGISTRATIONS`, ElementAnswers.java:159).
pub static REGISTRATIONS: LazyLock<Registrations> = LazyLock::new(platform_registrations);

/// ElementAnswers' `CLASSIFIERS` (ElementAnswers.java:252).
pub const CLASSIFIERS: [&str; 6] = [
    "meta::pure::metamodel::type::Class",
    "meta::pure::metamodel::type::Enumeration",
    "meta::pure::metamodel::relationship::Association",
    "meta::pure::mapping::Mapping",
    "meta::relational::metamodel::Database",
    "meta::pure::metamodel::type::Any",
];

const PAIRS: usize = 48;
const NAMES: usize = 32;
const ABSENT: &str = "__absent__";
const TABLE_LISTS: [&str; 4] = ["duplicates", "dangling", "conflicts", "memberWalls"];

/// A text being written: UTF-16 units, as Java's `StringBuilder`.
#[derive(Default)]
pub struct Out(pub Vec<u16>);

impl Out {
    pub fn s(&mut self, s: &str) -> &mut Out {
        self.0.extend(s.encode_utf16());
        self
    }
    pub fn j(&mut self, s: &JStr) -> &mut Out {
        self.0.extend_from_slice(&s.0);
        self
    }
    pub fn u(&mut self, u: &[u16]) -> &mut Out {
        self.0.extend_from_slice(u);
        self
    }
    /// A `T` or an `F`.
    fn tf(&mut self, b: bool) -> &mut Out {
        self.s(if b { "T" } else { "F" })
    }
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// A value's record-codec text, `exact` when its slot declares its very class.
fn json<T: Codec + ?Sized>(value: &T, exact: bool) -> Vec<u16> {
    units(&codec::write(value, exact))
}

/// `codec.write(s, String.class)`: a JSON string, or `null`.
fn json_string(s: Option<&JStr>) -> Vec<u16> {
    match s {
        None => units("null"),
        Some(s) => {
            let mut w = Writer::new();
            w.string(s);
            units(&w.finish())
        }
    }
}

/// `ElementAnswers.strings(xs)` (ElementAnswers.java:1023): `[` each string as the codec writes one `]`.
pub fn strings<'a>(xs: impl IntoIterator<Item = &'a JStr>) -> Vec<u16> {
    let mut w = Writer::new();
    w.raw("[");
    for (i, x) in xs.into_iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        w.string(x);
    }
    w.raw("]");
    units(&w.finish())
}

/// `ElementAnswers.nullableStrings(xs)` (ElementAnswers.java:1019).
fn nullable_strings(xs: Option<&Arc<[JStr]>>) -> Vec<u16> {
    match xs {
        None => units("null"),
        Some(xs) => strings(xs.iter()),
    }
}

/// `ElementAnswers.walls(walls)` (ElementAnswers.java:1034): `[[key,line],...]`.
pub fn walls_json(walls: &LinkedMap<JStr, JStr>) -> Vec<u16> {
    let mut w = Writer::new();
    w.raw("[");
    for (i, (k, v)) in walls.iter().enumerate() {
        if i > 0 {
            w.raw(",");
        }
        w.raw("[");
        w.string(k);
        w.raw(",");
        w.string(v);
        w.raw("]");
    }
    w.raw("]");
    units(&w.finish())
}

/// `ElementAnswers.error(e)` (ElementAnswers.java:1052): `E TAB class TAB phase TAB element TAB message` (`-` for no phase or element, `null` for no
/// message, as Java's concatenation of `getMessage()`).
pub fn error(e: &Thrown) -> Vec<u16> {
    let mut out = Out::default();
    out.s("E\t").s(e.java_class()).s("\t");
    match e {
        Thrown::Compile(c) => {
            out.s(c.phase.map_or("-", |p| p.name())).s("\t");
            match &c.element {
                Some(el) => out.u(el),
                None => out.s("-"),
            };
        }
        Thrown::Java { .. } => {
            out.s("-\t-");
        }
    }
    out.s("\t");
    match e.message() {
        Some(m) => out.j(&m),
        None => out.s("null"),
    };
    out.0
}

/// `ElementAnswers.cell(c)` (ElementAnswers.java:954): a lazy cell forced -- its answer, or its refusal's class and first line in its place.
fn cell(f: impl FnOnce() -> Result<Vec<u16>, Thrown>) -> Vec<u16> {
    match f() {
        Ok(v) => v,
        Err(e) => {
            let mut out = Out::default();
            out.s("R\t").s(e.java_class()).s("\t").j(&e.first_line());
            out.0
        }
    }
}

// =====================================================================
// The build: D, E, F
// =====================================================================

/// What a build hands the answer beside the model: E's model, the graph's own elements its first `own` (the rest are
/// the boot layer's very elements: ElementAnswers' identity filter, `BOOT_ELEMENTS`, keeps exactly these).
pub struct Built {
    pub normalized: lite_model::NormalizedModel,
    pub own: usize,
}

impl Built {
    /// The graph's own elements, in ingest order.
    pub fn own_elements(&self) -> &[PackageableElement] {
        &self.normalized.elements[..self.own]
    }
}

/// `ElementAnswers.build(parsed, walls)` (ElementAnswers.java:365): `resolveAlongside(parsed, bootFqns, walls)`, `normalizeWithSystem`, then
/// `PureModelContext.from(model, index, walls, boot.checked().without(adopted), registrations)`, one walls map (or
/// none, strict) through all three.
pub fn build<'m>(
    arena: &'m ModelArena,
    parsed: &ParsedModel,
    mut walls: Option<&mut LinkedMap<JStr, JStr>>,
) -> Result<(Model<'m>, Built), Thrown> {
    let resolved = resolver::resolve_alongside(parsed, boot_fqns(), walls.as_deref_mut())?;
    let Layer {
        model: normalized,
        index,
        adopted,
        own,
    } = normalize_with_system(&resolved, walls.as_deref_mut())?;
    let prior = boot_checked().without(&adopted);
    let model = Model::from(arena, &normalized, index, walls, Some(&prior), &REGISTRATIONS)?;
    Ok((model, Built { normalized, own }))
}

// =====================================================================
// The boot dump and the baselines
// =====================================================================

/// The empty text's answers every model shares (ElementAnswers' `Baseline`, ElementAnswers.java:249): subtracted from each ask's lists.
pub struct Baseline {
    universe: HashSet<JStr>,
    /// per [`CLASSIFIERS`] entry (an untracked one's: empty)
    classifiers: Vec<HashSet<JStr>>,
    /// per [`TABLE_LISTS`] entry
    lists: Vec<HashSet<JStr>>,
}

/// `ElementAnswers.tableLists(ctx)` (ElementAnswers.java:536): the declarations' duplicates, the implementations' dangling, conflicts and member
/// walls.
fn table_lists<'a>(model: &'a Model<'_>) -> [&'a [JStr]; 4] {
    [
        model.declarations().duplicates(),
        model.implementations().dangling(),
        model.implementations().conflicts(),
        model.implementations().member_walls(),
    ]
}

/// The universe's iteration as Java's `HashSet` holds it: each name once.
fn universe_set(model: &Model<'_>) -> HashSet<JStr> {
    model.resolution_universe().iter().cloned().collect()
}

/// `ElementAnswers.boot(codec)` (ElementAnswers.java:470): the empty text's model, built strict; its baselines; then `A`, every element's cell
/// (the boot layer's: C, N, F and I lines), the sorted universe, each classifier's instances and the tables' lists,
/// whole -- `answers.tsv.boot.txt`, its trailing newline included.
pub fn boot_text() -> (Vec<u16>, Baseline) {
    let parsed = lite_builtin::door::parse(&[], Dialect::LegendLite)
        .unwrap_or_else(|e| panic!("the empty text does not parse: {e:?}"));
    let arena = ModelArena::new();
    let (model, built) = build(&arena, &parsed, None).unwrap_or_else(|e| {
        panic!(
            "the empty text's model is refused: {}",
            String::from_utf16_lossy(&error(&e))
        )
    });
    let universe = universe_set(&model);
    let classifiers: Vec<HashSet<JStr>> = CLASSIFIERS
        .iter()
        .map(|c| {
            model
                .classifier_instances(&JStr::from(*c))
                .map(|xs| xs.into_iter().collect())
                .unwrap_or_default()
        })
        .collect();
    let lists: Vec<HashSet<JStr>> = table_lists(&model)
        .iter()
        .map(|l| l.iter().cloned().collect())
        .collect();
    let mut out = Out::default();
    out.s("A");
    answer(&mut out, &model, &built.normalized.elements, None);
    let mut sorted: Vec<JStr> = universe.iter().cloned().collect();
    sorted.sort();
    out.s("\nU\t").s(&sorted.len().to_string()).s("\t").u(&strings(&sorted));
    for c in CLASSIFIERS {
        out.s("\nX\t").s(c).s("\t");
        match model.classifier_instances(&JStr::from(c)) {
            None => out.s("null"),
            Some(xs) => out.s(&xs.len().to_string()).s("\t").u(&strings(&xs)),
        };
    }
    for (name, list) in TABLE_LISTS.iter().zip(table_lists(&model)) {
        out.s("\nL\t")
            .s(name)
            .s("\t")
            .s(&list.len().to_string())
            .s("\t")
            .u(&strings(list));
    }
    out.s("\n");
    (
        out.0,
        Baseline {
            universe,
            classifiers,
            lists,
        },
    )
}

// =====================================================================
// One ask
// =====================================================================

/// `ElementAnswers.run(parsed, walls, codec, mode)` (ElementAnswers.java:376): strict (no walls) or tolerant -- the build's error, or the walls
/// when a tolerant build has any (`W TAB [[key,line],...]`, then a newline), `A`, and the answer.
pub fn run(parsed: &ParsedModel, tolerant: bool, base: &Baseline) -> Vec<u16> {
    let arena = ModelArena::new();
    let mut walls: LinkedMap<JStr, JStr> = LinkedMap::new();
    let built = build(&arena, parsed, tolerant.then_some(&mut walls));
    match built {
        Err(e) => error(&e),
        Ok((model, built)) => {
            let mut out = Out::default();
            if !walls.is_empty() {
                out.s("W\t").u(&walls_json(&walls)).s("\n");
            }
            out.s("A");
            answer(&mut out, &model, built.own_elements(), Some(base));
            out.0
        }
    }
}

/// `ElementAnswers.form` (ElementAnswers.java:354): `S TAB strict NL T TAB tolerant`, the tolerant `=S` when equal.
pub fn form(s: &[u16], t: &[u16]) -> Vec<u16> {
    let mut out = Out::default();
    out.s("S\t").u(s).s("\nT\t");
    if t == s {
        out.s("=S");
    } else {
        out.u(t);
    }
    out.0
}

/// A `LinkedHashSet`: each value once, in first-insertion order.
struct Ordered<T> {
    list: Vec<T>,
    seen: HashSet<T>,
}

impl<T: Clone + Eq + std::hash::Hash> Ordered<T> {
    fn new() -> Ordered<T> {
        Ordered {
            list: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// `add`: whether it was not there.
    fn add(&mut self, v: T) -> bool {
        if self.seen.insert(v.clone()) {
            self.list.push(v);
            true
        } else {
            false
        }
    }
}

fn text(s: &str) -> JStr {
    JStr::from(s)
}

/// `ElementAnswers.typeNames(te, out)` (ElementAnswers.java:970): the type names a type expression spells.
fn type_names(te: &p::TypeExpression, out: &mut Ordered<JStr>) {
    match te {
        p::TypeExpression::TypeExpressionNameRef(nr) => {
            out.add(nr.name.clone());
        }
        p::TypeExpression::TypeExpressionGeneric(g) => {
            out.add(g.name.clone());
            for a in &g.arguments {
                type_names(a, out);
            }
        }
        p::TypeExpression::TypeExpressionFunctionType(ft) => {
            for tp in &ft.parameters {
                type_names(&tp.r#type, out);
            }
            type_names(&ft.result.r#type, out);
        }
        p::TypeExpression::TypeExpressionRelationType(rt) => {
            for c in &rt.columns {
                type_names(&c.r#type, out);
            }
        }
        p::TypeExpression::TypeExpressionSchemaAlgebra(sa) => {
            type_names(&sa.left, out);
            type_names(&sa.right, out);
        }
    }
}

/// `ElementAnswers.declaredNames` (ElementAnswers.java:804): the property names a class's declared reach names -- its own, then each
/// supertype's, depth first, each class once (through `findClassDefinition`).
fn declared_names(model: &Model<'_>, cd: &ClassDefinition, names: &mut Ordered<JStr>, seen: &mut HashSet<JStr>) {
    if !seen.insert(cd.qualified_name.clone()) {
        return;
    }
    for pd in cd.properties.iter() {
        names.add(pd.name.clone());
    }
    for dp in cd.derived_properties.iter() {
        names.add(dp.name.clone());
    }
    for sup in cd.super_classes.iter() {
        let head = match sup {
            p::TypeExpression::TypeExpressionNameRef(nr) => Some(&nr.name),
            p::TypeExpression::TypeExpressionGeneric(g) => Some(&g.name),
            _ => None,
        };
        if let Some(head) = head
            && let Some(sc) = model.find_class_definition(head)
        {
            declared_names(model, sc, names, seen);
        }
    }
}

/// Java's `d == f` on two functions: the same definition object.
fn identical(a: &Function, b: &Function) -> bool {
    match (a, b) {
        (Function::FunctionDefinition(x), Function::FunctionDefinition(y)) => same(x, y),
        (Function::NativeFunctionDefinition(x), Function::NativeFunctionDefinition(y)) => same(x, y),
        _ => false,
    }
}

/// `ElementAnswers.functionList(ctx, fqn, codec)` (ElementAnswers.java:928): each overload's view in candidate order, then ` same` when a second
/// ask hands out the same instances and every `findFunctionById` answer at this name is one of them, else ` apart`.
fn function_list(model: &Model<'_>, fqn: &JStr) -> Result<Vec<u16>, Thrown> {
    let first = model.find_function(fqn)?;
    let views: Vec<Vec<u16>> = first.iter().map(|tf| json(&views::function_view(tf), true)).collect();
    let again = model.find_function(fqn)?;
    let mut alike = again.len() == first.len();
    let mut i = 0;
    while alike && i < first.len() {
        alike = same(&again[i], &first[i]);
        i += 1;
    }
    for tf in first {
        for by_id in model.find_function_by_id(&tf.id.qualified)? {
            if !first.iter().any(|x| same(x, &by_id)) && by_id.qualified_name == *fqn {
                alike = false;
            }
        }
    }
    let mut out = Out::default();
    out.s("[").u(&views.join(&[u16::from(b',')][..])).s("]");
    out.s(if alike { " same" } else { " apart" });
    Ok(out.0)
}

/// `[` the views `]`.
fn function_views(fs: &[Arc<TypedFunction<'_>>]) -> Vec<u16> {
    let views: Vec<Vec<u16>> = fs.iter().map(|tf| json(&views::function_view(tf), true)).collect();
    let mut out = Out::default();
    out.s("[").u(&views.join(&[u16::from(b',')][..])).s("]");
    out.0
}

/// `ElementAnswers.row(r)` (ElementAnswers.java:998): an implementation-table row, as rust/oracles/builtin's PlatformTables writes one.
fn row(r: &Implementation) -> Vec<Vec<u16>> {
    fn names<T: Copy>(values: &[T], name: impl Fn(T) -> &'static str) -> Vec<u16> {
        units(&values.iter().map(|&v| name(v)).collect::<Vec<_>>().join(","))
    }
    match r {
        Implementation::Form(f) => vec![
            units("Form"),
            units(f.form().name()),
            names(f.also_lowered(), lite_platform::Position::name),
            names(f.also_families(), lite_builtin::native_fn::Family::name),
        ],
        Implementation::Intrinsic(n) => vec![
            units("Intrinsic"),
            names(n.positions(), lite_platform::Position::name),
            names(n.feature_overrides(), lite_platform::Feature::name),
            names(n.families(), lite_builtin::native_fn::Family::name),
        ],
        Implementation::Body | Implementation::PlatformPure | Implementation::Unimplemented => vec![units(r.kind())],
        Implementation::Refused(x) => vec![units("Refused"), units(x.reason.name()), x.why.0.clone()],
    }
}

fn tabbed(fields: Vec<Vec<u16>>) -> Vec<u16> {
    fields.join(&[u16::from(b'\t')][..])
}

/// `{@code List<KeyThread>}` as the codec writes it untagged, or `null`.
fn key_threads(xs: Option<&Arc<[Arc<KeyThread>]>>) -> Vec<u16> {
    match xs {
        None => units("null"),
        Some(xs) => {
            let mut w = Writer::new();
            w.raw("[");
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    w.raw(",");
                }
                x.write(&mut w, true);
            }
            w.raw("]");
            units(&w.finish())
        }
    }
}

/// `ElementAnswers.mappingFacts` (ElementAnswers.java:823): the mapping fact readers over the classes, sets and associations the mapping binds,
/// its facts key, and its poisons name, each with one absent.
fn mapping_facts(out: &mut Out, model: &Model<'_>, m: &MappingDefinition) {
    let fqn = &m.qualified_name;
    let f = &m.facts;
    let mut classes: Ordered<JStr> = Ordered::new();
    let mut sets: Ordered<(JStr, JStr)> = Ordered::new();
    for cb in m.class_bindings.iter() {
        classes.add(cb.class_fqn().clone());
        if let Some(set) = cb.set_id() {
            sets.add((cb.class_fqn().clone(), set.clone()));
        }
    }
    for (k, _) in f.union_members.iter() {
        classes.add(k.clone());
    }
    for (k, _) in f.mixed_unions.iter() {
        classes.add(k.clone());
    }
    for (k, _) in f.union_key_threads.iter() {
        classes.add(k.clone());
    }
    for (k, _) in f.routed_target_classes.iter() {
        classes.add(k.clone());
    }
    let mut associations: Ordered<JStr> = Ordered::new();
    for ab in m.association_bindings.iter() {
        associations.add(ab.association_fqn.clone());
    }
    for (k, _) in f.poisons.iter() {
        match k {
            PoisonKey::PoisonKeyForClass(c) => {
                classes.add(c.class_fqn.clone());
            }
            PoisonKey::PoisonKeyForSet(s) => {
                sets.add((s.class_fqn.clone(), s.set_id.clone()));
            }
            PoisonKey::PoisonKeyForAssociation(a) => {
                associations.add(a.association_fqn.clone());
            }
        }
    }
    out.s("\nG\t")
        .j(fqn)
        .s("\tlegacy\t")
        .tf(model.find_legacy_mapping(fqn).is_some());
    classes.add(text(ABSENT));
    for c in &classes.list {
        out.s("\nG\t")
            .j(fqn)
            .s("\tclass\t")
            .j(c)
            .s("\t")
            .u(&nullable_strings(model.union_member_classes(fqn, c)))
            .s("\t")
            .u(&nullable_strings(model.mixed_union_members(fqn, c)))
            .s("\t")
            .u(&key_threads(model.union_key_threads(fqn, c)))
            .s("\t")
            .u(&json_string(model.mapping_poison(fqn, c)));
        let mut props: Vec<JStr> = linked::get(&f.routed_target_classes, c)
            .map(|routed| routed.iter().map(|(p, _)| p.clone()).collect())
            .unwrap_or_default();
        props.push(text(ABSENT));
        for p in &props {
            out.s("\nG\t")
                .j(fqn)
                .s("\troute\t")
                .j(c)
                .s("\t")
                .j(p)
                .s("\t")
                .u(&json_string(model.routed_target_class(fqn, c, p)));
        }
    }
    sets.add((text(ABSENT), text(ABSENT)));
    for (c, s) in &sets.list {
        out.s("\nG\t")
            .j(fqn)
            .s("\tset\t")
            .j(c)
            .s("\t")
            .j(s)
            .s("\t")
            .u(&json_string(model.mapping_set_poison(fqn, c, s)));
    }
    associations.add(text(ABSENT));
    for a in &associations.list {
        out.s("\nG\t")
            .j(fqn)
            .s("\tassociation\t")
            .j(a)
            .s("\t")
            .u(&json_string(model.mapping_association_poison(fqn, a)));
    }
}

/// `ElementAnswers.answer(out, ctx, own, codec, mode, base)` (ElementAnswers.java:553): one line per question about `own` (the graph's own
/// elements, or the boot layer's for the boot dump, `base` none: their cells alone), in ingest order.
pub fn answer(out: &mut Out, model: &Model<'_>, own: &[PackageableElement], base: Option<&Baseline>) {
    let scratch = QueryArena::new();
    let mut classes: Vec<&Arc<ClassDefinition>> = Vec::new();
    let mut functions: Vec<Function> = Vec::new();
    let mut function_fqns: Ordered<JStr> = Ordered::new();
    let mut associations: Vec<&Arc<lite_model::AssociationDefinition>> = Vec::new();
    let mut mappings: Vec<&Arc<MappingDefinition>> = Vec::new();
    let mut databases: Vec<&Arc<lite_model::DatabaseDefinition>> = Vec::new();
    let mut names: Ordered<JStr> = Ordered::new();
    for el in own {
        names.add(el.qualified_name().clone());
        match el {
            PackageableElement::ClassDefinition(cd) => {
                classes.push(cd);
                out.s("\nC\t").j(&cd.qualified_name).s("\t").u(&cell(|| {
                    Ok(match model.find_class(&cd.qualified_name)? {
                        None => units("none"),
                        Some(tc) => json(&views::class_view(tc), true),
                    })
                }));
                for sup in cd.super_classes.iter() {
                    type_names(sup, &mut names);
                }
                for pd in cd.properties.iter() {
                    type_names(&pd.r#type, &mut names);
                }
                for dp in cd.derived_properties.iter() {
                    type_names(&dp.r#type, &mut names);
                    for p in &dp.parameters {
                        type_names(&p.r#type, &mut names);
                    }
                }
            }
            PackageableElement::EnumDefinition(ed) => {
                out.s("\nN\t").j(&ed.qualified_name).s("\t").u(&cell(|| {
                    Ok(match model.find_enum(&ed.qualified_name) {
                        None => units("none"),
                        Some(te) => json(&views::typed_enum(te), true),
                    })
                }));
            }
            PackageableElement::FunctionDefinition(_) | PackageableElement::NativeFunctionDefinition(_) => {
                let f = el.as_function().expect("a function element is a Function");
                for p in f.parameters().iter() {
                    type_names(&p.r#type, &mut names);
                }
                type_names(f.return_type(), &mut names);
                let fqn = f.qualified_name().clone();
                if function_fqns.add(fqn.clone()) {
                    out.s("\nF\t").j(&fqn).s("\t").u(&cell(|| function_list(model, &fqn)));
                }
                let id = FunctionId::of(&f).qualified;
                out.s("\nI\t")
                    .j(&id)
                    .s("\t")
                    .u(&cell(|| Ok(function_views(&model.find_function_by_id(&id)?))));
                functions.push(f);
            }
            PackageableElement::AssociationDefinition(a) => {
                associations.push(a);
                type_names(&a.property1.target_class, &mut names);
                type_names(&a.property2.target_class, &mut names);
            }
            PackageableElement::MappingDefinition(m) => mappings.push(m),
            PackageableElement::DatabaseDefinition(db) => databases.push(db),
            _ => {}
        }
    }
    let Some(base) = base else {
        // the boot dump: the elements' cells alone
        return;
    };
    // P4: findProperty over the names each class's declared reach and the associations name, and one absent
    for cd in &classes {
        let mut declared: Ordered<JStr> = Ordered::new();
        declared_names(model, cd, &mut declared, &mut HashSet::new());
        for a in &associations {
            declared.add(a.property1.property_name.clone());
            declared.add(a.property2.property_name.clone());
        }
        let mut asked: Vec<JStr> = declared.list[..NAMES.min(declared.list.len())].to_vec();
        asked.push(text(ABSENT));
        for n in &asked {
            out.s("\nP\t").j(&cd.qualified_name).s("\t").j(n).s("\t").u(&cell(|| {
                Ok(match model.find_property(&scratch, &cd.qualified_name, n)? {
                    None => units("none"),
                    Some(p) => json(&views::property(&p), false),
                })
            }));
        }
    }
    // findType
    for n in &names.list {
        out.s("\nY\t").j(n).s("\t").u(&cell(|| {
            Ok(match model.find_type(&scratch, n) {
                None => units("none"),
                Some(t) => json(&views::ty(t), false),
            })
        }));
    }
    // P2: isSubtype
    let children: Vec<&JStr> = classes.iter().map(|cd| &cd.qualified_name).collect();
    let bounded = &children[..PAIRS.min(children.len())];
    let any = text("meta::pure::metamodel::type::Any");
    let nil = text("meta::pure::metamodel::type::Nil");
    let mut parents: Vec<&JStr> = bounded.to_vec();
    parents.push(&any);
    parents.push(&nil);
    for c in bounded {
        for p in &parents {
            out.s("\nS\t")
                .j(c)
                .s("\t")
                .j(p)
                .s("\t")
                .u(&cell(|| Ok(units(if model.is_subtype(c, p)? { "T" } else { "F" }))));
        }
    }
    // P8: the declaration and implementation tables
    let decl = model.declarations();
    let imp = model.implementations();
    let mut fqns_asked: Ordered<JStr> = Ordered::new();
    for f in &functions {
        let id = FunctionId::of(f);
        let entry = match decl.get(&id) {
            None => units("null"),
            Some(d) if identical(d, f) => units("self"),
            Some(d) => {
                let mut e = Out::default();
                e.s("other\t").j(&FunctionId::of(d).qualified);
                e.0
            }
        };
        out.s("\nD\t").j(&id.qualified).s("\t").u(&entry).s("\t");
        match imp.of(&id) {
            None => out.s("null"),
            Some(r) => out.u(&tabbed(row(r))),
        };
        out.s("\t")
            .s(if imp.runs_by_rule(Some(f)) { "true" } else { "false" })
            .s("\t")
            .u(&tabbed(row(imp.row_of(f))));
        let fqn = f.qualified_name();
        if fqns_asked.add(fqn.clone()) {
            let ids: Vec<Vec<u16>> = decl.at(fqn).iter().map(|x| FunctionId::of(x).qualified.0).collect();
            out.s("\nD@\t").j(fqn).s("\t").u(&ids.join(&[u16::from(b',')][..]));
        }
    }
    for (k, (name, list)) in TABLE_LISTS.iter().zip(table_lists(model)).enumerate() {
        let extra: Vec<&JStr> = list.iter().filter(|s| !base.lists[k].contains(*s)).collect();
        out.s("\nL\t")
            .s(name)
            .s("\t")
            .s(&list.len().to_string())
            .s("\t")
            .u(&strings(extra));
    }
    // P6: the mapping fact readers
    for m in &mappings {
        mapping_facts(out, model, m);
    }
    // P10: findTable and findTableMilestoning over the graph's databases' tables, findViewFunction over their views
    // (their default schema), and isExecutionContextElement over the graph's elements
    for db in &databases {
        for td in db.tables.iter() {
            out.s("\nB\t")
                .j(&db.qualified_name)
                .s("\t")
                .j(&td.name)
                .s("\t")
                .u(&cell(|| {
                    Ok(match model.find_table(&scratch, &db.qualified_name, &td.name)? {
                        None => units("none"),
                        Some(rt) => json(&views::relation_type(rt), true),
                    })
                }))
                .s("\t")
                .u(&cell(|| {
                    Ok(match model.find_table_milestoning(&db.qualified_name, &td.name) {
                        None => units("null"),
                        Some(m) => json(&**m, true),
                    })
                }));
        }
        for vd in db.views.iter() {
            out.s("\nV\t")
                .j(&db.qualified_name)
                .s("\t")
                .j(&vd.name)
                .s("\t")
                .u(&cell(|| {
                    Ok(match model.find_view_function(&db.qualified_name, &vd.name)? {
                        None => units("none"),
                        Some(tf) => json(&views::function_view(tf), true),
                    })
                }));
        }
    }
    for el in own {
        out.s("\nE\t")
            .j(el.qualified_name())
            .s("\t")
            .tf(model.is_execution_context_element(el.qualified_name()));
    }
    // P5: the name universe
    let mut asked: Ordered<JStr> = Ordered::new();
    for n in &names.list {
        asked.add(n.clone());
    }
    for f in &functions {
        asked.add(FunctionId::of(f).qualified);
    }
    asked.add(text("absent::Name"));
    for n in &asked.list {
        out.s("\nK\t").j(n).s("\t").tf(model.resolution_universe_contains(n));
    }
    let mut packages: Ordered<JStr> = Ordered::new();
    let sep = units("::");
    for el in own {
        let fqn = &el.qualified_name().0;
        let mut at = 0;
        while let Some(i) = fqn[at..].windows(2).position(|w| w == &sep[..]).map(|i| i + at) {
            packages.add(JStr::from(&fqn[..i]));
            at = i + 2;
        }
        packages.add(el.qualified_name().clone());
    }
    packages.add(text("absent"));
    for p in &packages.list {
        out.s("\nQ\t").j(p).s("\t").tf(model.is_package(p));
    }
    let mut sorted: Vec<JStr> = universe_set(model).into_iter().collect();
    sorted.sort();
    let extra: Vec<&JStr> = sorted.iter().filter(|s| !base.universe.contains(*s)).collect();
    out.s("\nU\t").s(&sorted.len().to_string()).s("\t").u(&strings(extra));
    // P9: the metamodel registry
    for (k, c) in CLASSIFIERS.iter().enumerate() {
        let c = text(c);
        out.s("\nX\t").j(&c).s("\t").tf(model.tracks_classifier(&c)).s("\t");
        match model.classifier_instances(&c) {
            None => {
                out.s("null");
            }
            Some(xs) => {
                let more: Vec<&JStr> = xs.iter().filter(|s| !base.classifiers[k].contains(*s)).collect();
                out.s(&xs.len().to_string()).s("\t").u(&strings(more));
            }
        }
    }
}
