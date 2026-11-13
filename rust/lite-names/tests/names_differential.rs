// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The names differential (docs/rust-port/PHASE3.md, the names slice, 2026-11-12): what step D hands on, written by
//! this crate exactly as core's `frontoracle/NameAnswers.java` writes it over the live Java, hashed and compared ask
//! by ask -- every model text of the front oracle at every dialect it is asked at (resolved strict and tolerant,
//! alongside the boot layer and bare, the association adoption strict and tolerant, the index asked every question E
//! and F ask it), and every (model, query) pair of the identity oracle whose texts the front oracle holds (the query
//! resolved with no model, and against the model's universe with no imports and with the model's own). The canonical
//! text's grammar is `NameAnswers`' javadoc; this file writes the same text, section by section.
//!
//! The names follow-up (2026-11-12) adds two more, written by the same Java run: per model ask, the **stores** form --
//! what the helpers the owner's ruling moved into this crate answer over the same index (`MilestoningStrategy` for every
//! class's stereotypes, `StoreCompiler.columnType` for every table column, `ViewSignatures` for every view,
//! `RelationalTypeInference` for every view column, `KnowledgeLayer.columnKind` for every column of every table or view
//! spelling) -- and per call shape the **calls** form, what `Calls`' builder makes of it (the shape spelled in the
//! oracle's line, so the same call is built here). A form of their own each, so the models' forms frozen before are
//! unchanged.
//!
//! Inputs: `LITE_NAMES_QUESTIONS` (rust/oracles/front/questions.jsonl.zst, decompressed), `LITE_NAMES_MODELS`,
//! `LITE_NAMES_QUERIES`, `LITE_NAMES_STORES` and `LITE_NAMES_CALLS` (Java's answers: rust/oracles/names/, decompressed;
//! the queries file names its pairs, which Java read from the identity oracle's keys, and the calls file its shapes).
//! `LITE_NAMES_LIMIT` asks only the first N of each; `LITE_NAMES_MISMATCHES` names a directory each mismatch's Rust text
//! is written to. Every ask must agree: a difference is a defect.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

use lite_base::{Addr, Identity, QueryArena};
use lite_front::codec::{self, Codec, Writer};
use lite_front::doors::{Dialect, JavaError, on_parse_stack};
use lite_front::sha256::sha256_utf16le;
use lite_front::wire;
use lite_model::linked::LinkedMap;
use lite_model::protocol as p;
use lite_model::{ClassMapping, Function, ImportScope, JStr, PackageableElement as E, ParsedModel};
use lite_names::milestoning::{Dimension, MilestoningStrategy};
use lite_names::model_index::{ModelIndex, StoreLookups};
use lite_names::resolver::{self, IndexUniverse};
use lite_names::{
    Thrown, calls, derived_props, knowledge, lattice, relational_kinds, relational_type_inference, store_compiler,
    view_signatures,
};
use lite_platform::CoreFn;
use lite_types::{RelationType, Type};

const SHOWN: usize = 25;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} names the file to read"))
}

/// A form being written: UTF-16 units, as Java's `StringBuilder`.
#[derive(Default)]
struct F(Vec<u16>);

impl F {
    fn s(&mut self, s: &str) -> &mut F {
        self.0.extend(s.encode_utf16());
        self
    }
    fn j(&mut self, s: &JStr) -> &mut F {
        self.0.extend_from_slice(&s.0);
        self
    }
    fn u(&mut self, u: &[u16]) -> &mut F {
        self.0.extend_from_slice(u);
        self
    }
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn joined(items: impl IntoIterator<Item = Vec<u16>>) -> Vec<u16> {
    let mut out = Vec::new();
    for (i, x) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(u16::from(b','));
        }
        out.extend(x);
    }
    out
}

fn jstrs<'a>(items: impl IntoIterator<Item = &'a JStr>) -> Vec<u16> {
    joined(items.into_iter().map(|s| s.0.clone()))
}

/// `E TAB class TAB phase TAB element TAB message`.
fn error(e: &Thrown) -> Vec<u16> {
    let mut f = F::default();
    f.s("E\t").s(e.java_class()).s("\t");
    match e {
        Thrown::Compile(c) => {
            f.s(c.phase.map_or("-", |p| p.name())).s("\t");
            match &c.element {
                Some(el) => f.u(el),
                None => f.s("-"),
            };
        }
        Thrown::Java { .. } => {
            f.s("-\t-");
        }
    }
    f.s("\t");
    match e.message() {
        Some(m) => f.j(&m),
        None => f.s("null"),
    };
    f.0
}

fn java_error(e: &JavaError) -> Vec<u16> {
    let mut f = F::default();
    f.s("E\t").s(&e.class).s("\t-\t-\t");
    match &e.message {
        Some(m) => f.j(m),
        None => f.s("null"),
    };
    f.0
}

fn fail(e: &Thrown) -> Vec<u16> {
    let mut f = F::default();
    f.s("!").s(e.java_class()).s(": ");
    match e.message() {
        Some(m) => f.j(&m),
        None => f.s("null"),
    };
    f.0
}

fn json<T: Codec + ?Sized>(v: &T, exact: bool) -> Vec<u16> {
    units(&codec::write(v, exact))
}

fn model_json(m: &ParsedModel) -> Vec<u16> {
    json(m, true)
}

fn strict(r: Result<ParsedModel, Thrown>, out: &mut Option<ParsedModel>) -> Vec<u16> {
    match r {
        Ok(m) => {
            let mut f = F::default();
            f.s("A\t").u(&model_json(&m));
            *out = Some(m);
            f.0
        }
        Err(e) => error(&e),
    }
}

fn walls_json(walls: &LinkedMap<JStr, JStr>) -> Vec<u16> {
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

fn tolerant(r: Result<ParsedModel, Thrown>, walls: &LinkedMap<JStr, JStr>, out: &mut Option<ParsedModel>) -> Vec<u16> {
    match r {
        Ok(m) => {
            let mut f = F::default();
            if !walls.is_empty() {
                f.u(&walls_json(walls)).s("\t");
            }
            f.s("A\t").u(&model_json(&m));
            *out = Some(m);
            f.0
        }
        Err(e) => error(&e),
    }
}

fn same(text: Vec<u16>, label: &str, earlier: &[u16]) -> Vec<u16> {
    if text == earlier {
        units(&format!("={label}"))
    } else {
        text
    }
}

fn boot_fqns() -> HashSet<JStr> {
    let mut out: HashSet<JStr> = lite_builtin::system_metamodel::element_fqns().iter().cloned().collect();
    out.extend(lite_builtin::prelude::prelude().element_fqns().iter().cloned());
    out
}

/// A model resolved, adopted and indexed: `NameAnswers.modelForm`; and the stores form over the same index (`-` when
/// there is none).
fn model_form(parsed: &ParsedModel, boot: &HashSet<JStr>) -> (Vec<u16>, Vec<u16>) {
    let mut stores = units("-");
    let mut f = F::default();
    let mut b = None;
    let b_text = strict(resolver::resolve_alongside(parsed, boot, None), &mut b);
    f.s("B\t").u(&b_text);
    let mut t_walls = LinkedMap::new();
    let mut t = None;
    let r = resolver::resolve_alongside(parsed, boot, Some(&mut t_walls));
    let t_text = tolerant(r, &t_walls, &mut t);
    f.s("\nT\t").u(&same(t_text.clone(), "B", &b_text));
    let mut ignored = None;
    let p_text = strict(resolver::resolve(parsed), &mut ignored);
    f.s("\nP\t").u(&same(p_text, "B", &b_text));
    let mut q_walls = LinkedMap::new();
    let r = resolver::resolve_tolerant(parsed, &mut q_walls);
    let q_text = tolerant(r, &q_walls, &mut ignored);
    f.s("\nQ\t").u(&same(q_text, "T", &t_text));
    let k_text = match &b {
        None => units("-"),
        Some(bm) => strict(
            knowledge::adopt_association_qualified_properties(bm, None),
            &mut ignored,
        ),
    };
    f.s("\nK\t").u(&same(k_text, "B", &b_text));
    let mut l = None;
    let l_text = match &t {
        None => units("-"),
        Some(tm) => {
            let mut l_walls = t_walls.clone();
            let r = knowledge::adopt_association_qualified_properties(tm, Some(&mut l_walls));
            tolerant(r, &l_walls, &mut l)
        }
    };
    f.s("\nL\t").u(&same(l_text, "T", &t_text));
    f.s("\nX");
    if let Some(lm) = &l {
        match ModelIndex::from(lm) {
            Err(e) => {
                f.s("\t").u(&error(&e));
            }
            Ok(ix) => {
                f.s("\n");
                index(&mut f, lm, &ix);
                stores = stores_form(lm, &ix);
            }
        }
    }
    (f.0, stores)
}

// =====================================================================
// The index
// =====================================================================

fn el_addr(el: &E) -> Addr {
    match el {
        E::ClassDefinition(x) => x.address(),
        E::CleanSheetMappingDefinition(x) => x.address(),
        E::AssociationDefinition(x) => x.address(),
        E::EnumDefinition(x) => x.address(),
        E::ProfileDefinition(x) => x.address(),
        E::FunctionDefinition(x) => x.address(),
        E::NativeFunctionDefinition(x) => x.address(),
        E::ServiceDefinition(x) => x.address(),
        E::RuntimeDefinition(x) => x.address(),
        E::ConnectionDefinition(x) => x.address(),
        E::ModelConnectionDefinition(x) => x.address(),
        E::ModelChainConnectionDefinition(x) => x.address(),
        E::ExecutionEnvironmentDefinition(x) => x.address(),
        E::DataSpaceDefinition(x) => x.address(),
        E::MeasureDefinition(x) => x.address(),
        E::PersistenceDefinition(x) => x.address(),
        E::PersistenceContextDefinition(x) => x.address(),
        E::FunctionActivatorDefinition(x) => x.address(),
        E::GenericSectionElementDefinition(x) => x.address(),
        E::DatabaseDefinition(x) => x.address(),
        E::MappingDefinition(x) => x.address(),
        E::LegacyMappingDefinition(x) => x.address(),
        E::PrimitiveExtensionDefinition(x) => x.address(),
        E::DataDefinition(x) => x.address(),
        E::OpaqueElementDefinition(x) => x.address(),
    }
}

fn cm_addr(cm: &ClassMapping) -> Addr {
    match cm {
        ClassMapping::ClassMappingRelational(x) => x.address(),
        ClassMapping::ClassMappingPure(x) => x.address(),
        ClassMapping::ClassMappingUnion(x) => x.address(),
        ClassMapping::ClassMappingRelationFunction(x) => x.address(),
        ClassMapping::ClassMappingInheritance(x) => x.address(),
    }
}

fn fn_addr(f: &Function) -> Addr {
    match f {
        Function::FunctionDefinition(x) => x.address(),
        Function::NativeFunctionDefinition(x) => x.address(),
    }
}

/// `NameAnswers.labels`: every record the model holds, by its place; first place wins.
struct Labels(HashMap<Addr, String>);

impl Labels {
    fn put(&mut self, a: Addr, l: String) {
        self.0.entry(a).or_insert(l);
    }

    fn of(m: &ParsedModel) -> Labels {
        let mut labels = Labels(HashMap::new());
        for (i, el) in m.elements.iter().enumerate() {
            labels.put(el_addr(el), format!("e{i}"));
        }
        for (i, el) in m.elements.iter().enumerate() {
            let e = format!("e{i}");
            match el {
                E::RuntimeDefinition(rd) => {
                    for (j, c) in rd.inline_connections.iter().enumerate() {
                        labels.put(el_addr(c), format!("{e}.c{j}"));
                    }
                }
                E::AssociationDefinition(ad) => {
                    labels.put(ad.property1.address(), format!("{e}.1"));
                    labels.put(ad.property2.address(), format!("{e}.2"));
                }
                E::DatabaseDefinition(db) => {
                    for (j, x) in db.tables.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.t{j}"));
                    }
                    for (j, x) in db.views.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.v{j}"));
                    }
                    for (j, x) in db.tabular_functions.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.f{j}"));
                    }
                    for (j, s) in db.schemas.iter().enumerate() {
                        for (k, x) in s.tables.iter().enumerate() {
                            labels.put(x.address(), format!("{e}.s{j}.t{k}"));
                        }
                        for (k, x) in s.views.iter().enumerate() {
                            labels.put(x.address(), format!("{e}.s{j}.v{k}"));
                        }
                        for (k, x) in s.tabular_functions.iter().enumerate() {
                            labels.put(x.address(), format!("{e}.s{j}.f{k}"));
                        }
                    }
                    for (j, x) in db.joins.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.j{j}"));
                    }
                    for (j, x) in db.filters.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.g{j}"));
                    }
                    for (j, x) in db.multi_grain_filters.iter().enumerate() {
                        labels.put(x.address(), format!("{e}.m{j}"));
                    }
                }
                E::LegacyMappingDefinition(lm) => {
                    for (j, cm) in lm.class_mappings.iter().enumerate() {
                        labels.put(cm_addr(cm), format!("{e}.cm{j}"));
                    }
                }
                _ => {}
            }
        }
        labels
    }

    fn get(&self, a: Option<Addr>) -> Vec<u16> {
        match a {
            None => units("-"),
            Some(a) => units(self.0.get(&a).map_or("n", String::as_str)),
        }
    }

    fn arc<T>(&self, x: Option<&Arc<T>>) -> Vec<u16> {
        self.get(x.map(|x| x.address()))
    }

    fn all<'a, T: 'a>(&self, xs: impl IntoIterator<Item = &'a Arc<T>>) -> Vec<u16> {
        joined(xs.into_iter().map(|x| self.arc(Some(x))))
    }
}

fn b(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}

fn attempt(r: Result<Vec<u16>, Thrown>) -> Vec<u16> {
    r.unwrap_or_else(|e| fail(&e))
}

fn index(f: &mut F, m: &ParsedModel, ix: &ModelIndex) {
    let labels = Labels::of(m);
    for (k, v) in ix.duplicate_elements() {
        f.s("d\t").j(k).s("\t").j(v).s("\n");
    }
    f.s("o\tclasses\t").u(&labels.all(ix.classes())).s("\n");
    f.s("o\tassociations\t").u(&labels.all(ix.associations())).s("\n");
    f.s("o\tfunctions\t")
        .u(&joined(ix.functions().map(|x| labels.get(Some(fn_addr(x))))))
        .s("\n");
    f.s("o\tdatabases\t").u(&labels.all(ix.databases())).s("\n");
    f.s("o\tdataElements\t").u(&labels.all(ix.data_elements())).s("\n");
    f.s("o\tmappings\t").u(&labels.all(ix.mappings())).s("\n");
    f.s("o\tlegacyMappings\t").u(&labels.all(ix.legacy_mappings())).s("\n");
    f.s("o\tenums\t").u(&labels.all(ix.enums())).s("\n");
    f.s("o\tmeasures\t").u(&labels.all(ix.measures())).s("\n");
    f.s("o\truntimes\t").u(&labels.all(ix.runtimes())).s("\n");
    f.s("o\tservices\t").u(&labels.all(ix.services())).s("\n");
    f.s("o\tdataSpaces\t").u(&labels.all(ix.data_spaces())).s("\n");
    f.s("o\tconnections\t").u(&labels.all(ix.connections())).s("\n");
    f.s("x\t").u(&jstrs(ix.primitive_extension_fqns())).s("\n");

    let k = ix.knowledge();
    let els = &m.elements;
    let mut keys: LinkedMap<JStr, ()> = LinkedMap::new();
    for el in els.iter() {
        keys.put_if_absent(el.qualified_name().clone(), ());
    }
    for el in els.iter() {
        if let E::RuntimeDefinition(rd) = el {
            for c in rd.inline_connections.iter() {
                keys.put_if_absent(c.qualified_name().clone(), ());
            }
        }
    }
    for el in els.iter() {
        match el {
            E::ClassDefinition(cd) => {
                for s in cd.super_classes.iter() {
                    if let Some(raw) = s.raw_class_name() {
                        keys.put_if_absent(raw.clone(), ());
                    }
                }
            }
            E::AssociationDefinition(ad) => {
                for end in [&ad.property1, &ad.property2] {
                    if let Some(raw) = end.target_class.raw_class_name() {
                        keys.put_if_absent(raw.clone(), ());
                    }
                }
            }
            E::DatabaseDefinition(db) => {
                for inc in db.includes.iter() {
                    keys.put_if_absent(inc.clone(), ());
                }
            }
            _ => {}
        }
    }
    keys.put_if_absent(JStr::from("absent::Name"), ());
    keys.put_if_absent(JStr::from(""), ());
    let keys: Vec<JStr> = keys.iter().map(|(k, _)| k.clone()).collect();
    for key in &keys {
        f.s("k\t").j(key);
        f.s("\t").u(&labels.arc(ix.find_class(key)));
        f.s("\t").u(&labels.arc(ix.find_association(key)));
        f.s("\t").u(&labels.arc(ix.find_enum(key)));
        f.s("\t").u(&labels.arc(ix.find_profile(key)));
        f.s("\t").u(&labels.arc(ix.find_measure(key)));
        f.s("\t").u(&labels.arc(ix.find_database(key)));
        f.s("\t").u(&labels.arc(ix.find_mapping(key)));
        f.s("\t").u(&labels.arc(ix.find_legacy_mapping(key)));
        f.s("\t").u(&labels.arc(ix.find_service(key)));
        f.s("\t").u(&labels.arc(ix.find_data(key)));
        f.s("\t").u(&labels.arc(ix.find_data_space(key)));
        f.s("\t").u(&labels.arc(ix.find_runtime(key)));
        f.s("\t").u(&labels.arc(ix.find_connection(key)));
        f.s("\t").s(b(ix.is_model_connection(key)));
        f.s("\t").u(&joined(
            ix.find_function(key).iter().map(|x| labels.get(Some(fn_addr(x)))),
        ));
        f.s("\t").s(b(ix.has_element(key)));
        f.s("\t").s(b(ix.has_database_exact(key)));
        f.s("\t").s(ix.find_primitive_extension(key).map_or("-", |p| p.name()));
        f.s("\t")
            .u(&jstrs(ix.primitive_type_variables(key).iter().map(|pd| &pd.name)));
        f.s("\t").u(&jstrs(ix.direct_subclasses(key)));
        f.s("\n");
        let cdef = k.class_def(key);
        f.s("c\t").j(key);
        f.s("\t").u(&labels.arc(cdef));
        f.s("\t").u(&labels.arc(k.hierarchy_class(Some(key))));
        f.s("\t");
        match cdef {
            Some(c) => f.u(&jstrs(&k.super_class_fqns(c))),
            None => f.s("-"),
        };
        f.s("\t").u(&attempt(k.ancestors_and_self(key).map(|a| jstrs(&a))));
        f.s("\t").u(&jstrs(&k.subtree(key)));
        f.s("\t").u(&jstrs(&k.direct_subtypes(key)));
        f.s("\t").u(&attempt(k.lineage(key).map(|l| labels.all(&l))));
        f.s("\n");
        let parents: Vec<&JStr> = keys.iter().filter(|p| k.is_subtype(key, p)).collect();
        f.s("s\t").j(key).s("\t").u(&jstrs(parents)).s("\n");
        if let Some(hc) = k.hierarchy_class(Some(key)) {
            for name in probe_names(key, &k, els) {
                f.s("p\t").j(key).s("\t").j(&name);
                f.s("\t").u(&attempt(
                    k.property_type(Some(hc), &name)
                        .map(|t| t.map_or_else(|| units("-"), |t| json(&t, false))),
                ));
                f.s("\t").u(&attempt(
                    k.property_def(Some(hc), &name)
                        .map(|t| t.map_or_else(|| units("-"), |t| json(&*t, true))),
                ));
                f.s("\t").u(&attempt(
                    k.property_multiplicity(hc, &name)
                        .map(|t| t.map_or_else(|| units("-"), |t| json(&t, false))),
                ));
                f.s("\t").u(&attempt(
                    k.derived_inline(Some(hc), &name)
                        .map(|t| t.map_or_else(|| units("-"), |t| json(&t, true))),
                ));
                f.s("\t").u(&labels.arc(ix.find_association_end(key, &name)));
                f.s("\t")
                    .u(&attempt(ix.find_association_of(key, &name).map(|a| labels.arc(a))));
                f.s("\n");
            }
        }
    }
    for (i, el) in els.iter().enumerate() {
        if let E::ClassDefinition(cd) = el {
            for dp in cd.derived_properties.iter() {
                f.s(&format!("l\te{i}\t")).j(&dp.name).s("\t");
                f.u(&attempt(derived_props::lift(cd, dp).map(|fd| json(&fd, true))));
                f.s("\n");
            }
        }
    }
    let mut dbs: LinkedMap<JStr, ()> = LinkedMap::new();
    let mut spellings: LinkedMap<JStr, ()> = LinkedMap::new();
    let mut joins: LinkedMap<JStr, ()> = LinkedMap::new();
    let mut filters: LinkedMap<JStr, ()> = LinkedMap::new();
    let dot = |a: &JStr, b: &JStr| lite_model::cat!(a, ".", b);
    for el in els.iter() {
        if let E::DatabaseDefinition(db) = el {
            dbs.put_if_absent(db.qualified_name.clone(), ());
            for inc in db.includes.iter() {
                dbs.put_if_absent(inc.clone(), ());
            }
            for x in db.tables.iter() {
                spellings.put_if_absent(x.name.clone(), ());
                spellings.put_if_absent(lite_model::cat!("default.", &x.name), ());
            }
            for x in db.views.iter() {
                spellings.put_if_absent(x.name.clone(), ());
            }
            for x in db.tabular_functions.iter() {
                spellings.put_if_absent(x.name.clone(), ());
            }
            for s in db.schemas.iter() {
                for x in s.tables.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
                for x in s.views.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
                for x in s.tabular_functions.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
            }
            for x in db.joins.iter() {
                joins.put_if_absent(x.name.clone(), ());
            }
            for x in db.filters.iter() {
                filters.put_if_absent(x.name.clone(), ());
            }
            for x in db.multi_grain_filters.iter() {
                filters.put_if_absent(x.name.clone(), ());
            }
        }
    }
    dbs.put_if_absent(JStr::from("absent::Db"), ());
    spellings.put_if_absent(JStr::from("absent_T"), ());
    joins.put_if_absent(JStr::from("absent_J"), ());
    filters.put_if_absent(JStr::from("absent_F"), ());
    for (db, _) in dbs.iter() {
        let views = ix.views_of(db);
        f.s("v\t").j(db).s("\t");
        f.u(&joined(views.iter().map(|iv| {
            let mut x = labels.arc(Some(&iv.definition));
            x.push(u16::from(b'@'));
            x.extend_from_slice(&iv.lift.spelling.0);
            x
        })));
        f.s("\n");
        for iv in views {
            f.s("w\t").j(db).s("\t").u(&labels.arc(Some(&iv.definition)));
            f.s("\t")
                .u(&attempt(ix.view_main_table(db, &iv.definition).map(|t| t.0)));
            f.s("\t").u(&attempt(iv.lift.fqn().map(|t| t.0)));
            f.s("\n");
        }
        for (sp, _) in spellings.iter() {
            let td = ix.find_table_definition(db, sp);
            f.s("t\t").j(db).s("\t").j(sp);
            f.s("\t").u(&labels.arc(td));
            f.s("\t").u(&labels.arc(ix.find_own_table_definition(db, sp)));
            f.s("\t").u(&labels.arc(ix.find_view(db, sp)));
            f.s("\t");
            match ix.view_lift(db, sp) {
                Some(vl) => f.j(&vl.owner_db).s("/").j(&vl.spelling),
                None => f.s("-"),
            };
            f.s("\t").u(&labels.arc(k.table(Some(db), Some(sp)).as_ref()));
            f.s("\n");
            if let Some(td) = td {
                for c in td.columns.iter() {
                    let hit = k.column(Some(db), Some(sp), &c.name);
                    let at = hit
                        .as_ref()
                        .and_then(|h| td.columns.iter().position(|x| lite_base::same(x, h)))
                        .map_or(-1, |i| i as i64);
                    f.s("u\t")
                        .j(db)
                        .s("\t")
                        .j(sp)
                        .s("\t")
                        .j(&c.name)
                        .s(&format!("\t{at}\t"));
                    f.s(hit.map_or("-", |h| relational_kinds::pure_kind_of(&h.data_type)));
                    f.s("\n");
                }
            }
        }
        for (j, _) in joins.iter() {
            f.s("j\t")
                .j(db)
                .s("\t")
                .j(j)
                .s("\t")
                .u(&labels.arc(ix.find_join(Some(db), j)))
                .s("\n");
        }
        for (g, _) in filters.iter() {
            f.s("g\t")
                .j(db)
                .s("\t")
                .j(g)
                .s("\t")
                .u(&labels.arc(ix.find_filter(Some(db), g)))
                .s("\n");
        }
    }
    let mut set_ids: LinkedMap<JStr, ()> = LinkedMap::new();
    for el in els.iter() {
        if let E::LegacyMappingDefinition(lm) = el {
            for cm in lm.class_mappings.iter() {
                set_ids.put_if_absent(lite_model::names::set_id_of_class_mapping(cm), ());
            }
        }
    }
    set_ids.put_if_absent(JStr::from("absent_set"), ());
    for (id, _) in set_ids.iter() {
        f.s("q\t").j(id).s("\t");
        f.u(&joined(ix.legacy_sets_with_id(id).iter().map(|s| {
            let mut x = labels.arc(Some(&s.defining));
            x.push(u16::from(b'/'));
            x.extend(labels.get(Some(cm_addr(&s.set))));
            x
        })));
        f.s("\n");
    }
    for el in els.iter() {
        let key = ParsedModel::key_of(el);
        f.s("i\t")
            .j(&key)
            .s("\t")
            .u(&jstrs(ix.imports_of(&key).wildcards.iter()))
            .s("\n");
    }
    let mut ids: LinkedMap<JStr, ()> = LinkedMap::new();
    for el in els.iter() {
        if let Some(func) = el.as_function() {
            ids.put_if_absent(lite_model::function_id::mangle(&func), ());
        }
    }
    ids.put_if_absent(JStr::from("absent_id"), ());
    for (id, _) in ids.iter() {
        f.s("f\t").j(id).s("\t");
        f.u(&joined(
            ix.find_function_by_id(id).iter().map(|x| labels.get(Some(fn_addr(x)))),
        ));
        f.s("\t").s(b(ix.has_function_id(id))).s("\n");
    }
}

/// `NameAnswers.probeNames`.
fn probe_names(key: &JStr, k: &lite_names::Knowledge<'_>, els: &[E]) -> Vec<JStr> {
    let mut names: LinkedMap<JStr, ()> = LinkedMap::new();
    let line = k.ancestors_and_self(key).unwrap_or_else(|_| vec![key.clone()]);
    for a in &line {
        if let Some(d) = k.hierarchy_class(Some(a)) {
            for pd in d.properties.iter() {
                names.put_if_absent(pd.name.clone(), ());
            }
            for dp in d.derived_properties.iter() {
                names.put_if_absent(dp.name.clone(), ());
            }
        }
        for el in els {
            if let E::AssociationDefinition(ad) = el {
                if let p::TypeExpression::TypeExpressionNameRef(n) = &ad.property2.target_class
                    && &n.name == a
                {
                    names.put_if_absent(ad.property1.property_name.clone(), ());
                }
                if let p::TypeExpression::TypeExpressionNameRef(n) = &ad.property1.target_class
                    && &n.name == a
                {
                    names.put_if_absent(ad.property2.property_name.clone(), ());
                }
            }
        }
    }
    names.put_if_absent(JStr::from("absent$prop"), ());
    names.iter().map(|(n, _)| n.clone()).collect()
}

// =====================================================================
// The stores: the helpers E calls, moved into this crate (2026-11-12)
// =====================================================================

/// `NameAnswers.typeText`: a type as its variant's letter and its `typeName()`.
fn type_text(t: Type<'_>) -> String {
    let kind = match t {
        Type::Primitive(_) => "P",
        Type::PrecisionDecimal(_) => "D",
        Type::ClassType(_) => "C",
        Type::EnumType(_) => "E",
        _ => "?",
    };
    format!("{kind}:{}", t.type_name())
}

/// `NameAnswers.signatureText`.
fn signature_text(r: &RelationType<'_>) -> Vec<u16> {
    let mut s = String::new();
    for (i, c) in r.columns().iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(c.name);
        s.push('=');
        s.push_str(&type_text(c.r#type));
        s.push_str(&c.multiplicity.text());
    }
    s.push('|');
    s.push_str(&r.dynamic_columns().len().to_string());
    units(&s)
}

fn data_type_text(dt: Option<lite_model::RelationalDataType>) -> Vec<u16> {
    dt.map_or_else(|| units("-"), |dt| json(&dt, false))
}

/// `NameAnswers.storesForm`: what the store and class helpers E calls answer over the index `X` asks.
fn stores_form(m: &ParsedModel, ix: &ModelIndex) -> Vec<u16> {
    let mut f = F::default();
    let labels = Labels::of(m);
    let k = ix.knowledge();
    let els = &m.elements;
    let arena = QueryArena::new();
    for (i, el) in els.iter().enumerate() {
        if let E::ClassDefinition(cd) = el {
            for (j, st) in cd.stereotypes.iter().enumerate() {
                let s = MilestoningStrategy::of_stereotype_or_null(&st.profile_name, &st.stereotype_name);
                f.s(&format!("m\te{i}\t{j}\t"));
                match s {
                    None => f.s("-"),
                    Some(s) => f
                        .s(s.name())
                        .s("/")
                        .s(b(s.has(Dimension::Business)))
                        .s(b(s.has(Dimension::Processing))),
                };
                f.s("\n");
            }
        }
    }
    for el in els.iter() {
        if let E::DatabaseDefinition(db) = el {
            let mut tables: Vec<&Arc<lite_model::DatabaseDefinitionTableDefinition>> = db.tables.iter().collect();
            for s in db.schemas.iter() {
                tables.extend(s.tables.iter());
            }
            for td in tables {
                for c in td.columns.iter() {
                    f.s("z\t").u(&labels.arc(Some(td))).s("\t").j(&c.name).s("\t");
                    f.u(&attempt(
                        store_compiler::column_type(&c.data_type).map(|t| units(&type_text(t))),
                    ));
                    f.s("\n");
                }
            }
        }
    }
    let mut dbs: LinkedMap<JStr, ()> = LinkedMap::new();
    let mut spellings: LinkedMap<JStr, ()> = LinkedMap::new();
    let dot = |a: &JStr, b: &JStr| lite_model::cat!(a, ".", b);
    for el in els.iter() {
        if let E::DatabaseDefinition(db) = el {
            dbs.put_if_absent(db.qualified_name.clone(), ());
            for inc in db.includes.iter() {
                dbs.put_if_absent(inc.clone(), ());
            }
            for x in db.tables.iter() {
                spellings.put_if_absent(x.name.clone(), ());
                spellings.put_if_absent(lite_model::cat!("default.", &x.name), ());
            }
            for x in db.views.iter() {
                spellings.put_if_absent(x.name.clone(), ());
            }
            for x in db.tabular_functions.iter() {
                spellings.put_if_absent(x.name.clone(), ());
            }
            for s in db.schemas.iter() {
                for x in s.tables.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
                for x in s.views.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
                for x in s.tabular_functions.iter() {
                    spellings.put_if_absent(dot(&s.name, &x.name), ());
                }
            }
        }
    }
    dbs.put_if_absent(JStr::from("absent::Db"), ());
    spellings.put_if_absent(JStr::from("absent_T"), ());
    let store: &dyn StoreLookups = ix;
    for (db, _) in dbs.iter() {
        let scope = ix.find_database(db).map(|d| &**d);
        for iv in ix.views_of(db) {
            let view = &iv.definition;
            let vl = labels.arc(Some(view));
            f.s("y\t").j(db).s("\t").u(&vl).s("\t");
            f.u(&attempt(
                view_signatures::of(&arena, store, &iv.lift.owner_db, view).map(|r| signature_text(r)),
            ));
            f.s("\n");
            for (c, cm) in view.column_mappings.iter().enumerate() {
                let expr = &cm.expression;
                f.s("r\t").j(db).s("\t").u(&vl).s(&format!("\t{c}\t"));
                f.u(&attempt(
                    relational_type_inference::infer(expr, scope, Some(store)).map(data_type_text),
                ));
                f.s("\t");
                f.u(&attempt(
                    relational_type_inference::infer(expr, None, Some(store)).map(data_type_text),
                ));
                f.s("\t");
                f.u(&attempt(
                    relational_type_inference::infer(expr, scope, None).map(data_type_text),
                ));
                f.s("\n");
            }
        }
        for (sp, _) in spellings.iter() {
            let mut names: Vec<JStr> = Vec::new();
            if let Some(td) = ix.find_table_definition(db, sp) {
                names.extend(td.columns.iter().map(|c| c.name.clone()));
            } else if let Some(v) = ix.find_view(db, sp) {
                names.extend(v.column_mappings.iter().map(|c| c.name.clone()));
            }
            names.push(JStr::from("absent_C"));
            for name in &names {
                f.s("n\t").j(db).s("\t").j(sp).s("\t").j(name).s("\t");
                f.u(&attempt(
                    k.column_kind(db, sp, name).map(|kind| units(kind.unwrap_or("-"))),
                ));
                f.s("\n");
            }
        }
    }
    f.0
}

// =====================================================================
// The call builder (2026-11-12)
// =====================================================================

/// `NameAnswers.args`: `n` `CString`s `<prefix>0 ...`.
fn call_args(prefix: &str, n: usize) -> Vec<p::ValueSpecification> {
    (0..n)
        .map(|i| {
            p::ValueSpecification::CString(Arc::new(p::CString {
                value: JStr::from(format!("{prefix}{i}")),
                pos: None,
                multi_line: false,
            }))
        })
        .collect()
}

/// `NameAnswers.refs`: a record's names, comma-joined in the shape (none when empty).
fn refs(joined: &str) -> Vec<JStr> {
    if joined.is_empty() {
        Vec::new()
    } else {
        joined.split(',').map(JStr::from).collect()
    }
}

/// `NameAnswers.original`: an original call for `like` and `renamed`, its written forms by letter.
fn original(function: &str, n: usize, referents: &str, flags: &str) -> p::AppliedFunction {
    p::AppliedFunction {
        function: JStr::from(function),
        parameters: call_args("a", n).into(),
        referents: refs(referents).into(),
        pos: flags.contains('p').then(|| {
            Box::new(p::SourceInfo {
                source_id: JStr::from("src"),
                start_line: 1,
                start_column: 2,
                end_line: 3,
                end_column: 4,
            })
        }),
        property_call: flags.contains('c'),
        grouped: flags.contains('g'),
        infix: flags.contains('x'),
        island: flags.contains('i'),
        f_control: flags.contains('f').then(|| JStr::from("fctl")),
        owner_class: flags.contains('o').then(|| JStr::from("my::Own")),
    }
}

/// `NameAnswers.callForm`: one shape asked of its builder, `A` and the call's JSON.
fn call_form(s: &[&str]) -> Vec<u16> {
    let n = |i: usize| -> usize { s[i].parse().expect("an arity") };
    let name = JStr::from(s[1]);
    let af = match s[0] {
        "platform" => calls::platform(&name, call_args("a", n(2))),
        "form" => {
            let form = CoreFn::VALUES.into_iter().find(|f| f.name() == s[1]).expect("a form");
            calls::form(form, call_args("a", n(2)))
        }
        "exact" => calls::exact(&name, call_args("a", n(2))),
        "carrier" => calls::carrier(&name, call_args("a", n(2))),
        "infixRun" => calls::infix_run(&name, call_args("a", n(2))),
        "among" => calls::among(&name, &refs(s[2]), call_args("a", n(3))),
        "infixRunAmong" => calls::infix_run_among(&name, &refs(s[2]), call_args("a", n(3))),
        "renamed" => calls::renamed(
            &original(s[1], n(2), s[3], s[4]),
            &JStr::from(s[5]),
            call_args("b", n(6)),
        ),
        "like" => calls::like(&original(s[1], n(2), s[3], s[4]), call_args("b", n(5))),
        other => panic!("no builder {other}"),
    };
    let mut f = F::default();
    f.s("A\t")
        .u(&json(&p::ValueSpecification::AppliedFunction(Arc::new(af)), false));
    f.0
}

// =====================================================================
// Queries
// =====================================================================

/// A model's index for a query (its L elements, then the boot layer's resolved ones) and its union imports.
struct QueryModel {
    index: ModelIndex,
    imports: Arc<ImportScope>,
}

fn query_model(text: &[u16], boot: &HashSet<JStr>, boot_resolved: &ParsedModel) -> Option<QueryModel> {
    let parsed = lite_builtin::door::parse(text, Dialect::LegendLite).ok()?;
    let mut walls = LinkedMap::new();
    let resolved = resolver::resolve_alongside(&parsed, boot, Some(&mut walls)).ok()?;
    let adopted = knowledge::adopt_association_qualified_properties(&resolved, Some(&mut walls)).ok()?;
    let mut ix = ModelIndex::from(&adopted).ok()?;
    ix.add(&boot_resolved.elements).ok()?;
    Some(QueryModel {
        index: ix,
        imports: parsed.imports.clone(),
    })
}

fn query(r: Result<p::ValueSpecification, Thrown>) -> Vec<u16> {
    match r {
        Ok(v) => {
            let mut f = F::default();
            f.s("A\t").u(&json(&v, false));
            f.0
        }
        Err(e) => error(&e),
    }
}

/// `NameAnswers.queryForm`.
fn query_form(text: &[u16], model: Option<&QueryModel>) -> Vec<u16> {
    let q = match lite_front::doors::spec(text, Dialect::LegendLite) {
        Ok(q) => q,
        Err(e) => {
            let mut f = F::default();
            f.s("P\t").u(&java_error(&e));
            return f.0;
        }
    };
    let mut f = F::default();
    let n = query(resolver::resolve_query(&q));
    f.s("N\t").u(&n);
    let Some(m) = model else {
        f.s("\nM\t-");
        return f.0;
    };
    let u = IndexUniverse { index: &m.index };
    let m_text = query(resolver::resolve_query_in(&q, ImportScope::empty(), &u));
    f.s("\nM\t").u(&same(m_text.clone(), "N", &n));
    let i_text = query(resolver::resolve_query_in(&q, m.imports.clone(), &u));
    f.s("\nI\t").u(&same(i_text, "M", &m_text));
    f.0
}

// =====================================================================
// The differential
// =====================================================================

#[test]
#[ignore = "reads the front and identity oracles and the names oracle (LITE_NAMES_*): \
            //rust/lite-names:names_differential_test runs it"]
fn what_step_d_hands_on_answers_as_javas() {
    on_parse_stack(differential);
}

fn differential() {
    let limit: usize = std::env::var("LITE_NAMES_LIMIT").map_or(usize::MAX, |l| l.parse().unwrap());
    let dump = std::env::var("LITE_NAMES_MISMATCHES").ok();
    let boot = boot_fqns();
    let mut java = BufReader::new(File::open(env("LITE_NAMES_MODELS")).unwrap()).lines();
    let mut java_stores = BufReader::new(File::open(env("LITE_NAMES_STORES")).unwrap()).lines();
    let mut texts: HashMap<String, Vec<u16>> = HashMap::new();
    let (mut asked, mut matched, mut s_matched) = (0usize, 0usize, 0usize);
    let mut differ: Vec<String> = Vec::new();
    let mut s_differ: Vec<String> = Vec::new();
    for line in BufReader::new(File::open(env("LITE_NAMES_QUESTIONS")).unwrap()).lines() {
        let line = line.unwrap();
        let q = wire::parse(&line, 64).unwrap();
        let sha = q.get("sha").and_then(wire::Json::as_string).unwrap();
        let text = q.get("text").and_then(wire::Json::as_units).unwrap();
        texts.insert(sha.clone(), text.to_vec());
        for a in q.get("asks").and_then(wire::Json::as_array).unwrap() {
            let pair = a.as_array().unwrap();
            let (door, dialect_name) = (pair[0].as_string().unwrap(), pair[1].as_string().unwrap());
            if door != "pmcd" || asked >= limit {
                continue;
            }
            let dialect = Dialect::from_java(&dialect_name).expect("a dialect");
            let Ok(parsed) = lite_builtin::door::parse(text, dialect) else {
                continue;
            };
            asked += 1;
            let java_line = java
                .next()
                .unwrap_or_else(|| panic!("Java answered no model ask for {dialect_name} {sha}"))
                .unwrap();
            let f: Vec<&str> = java_line.split('\t').collect();
            assert_eq!(
                f.len(),
                3,
                "a line of the models oracle is sha, dialect, hash: {java_line}"
            );
            assert!(
                f[0] == sha && f[1] == dialect_name,
                "the models oracle is out of step at {sha} {dialect_name}: {java_line}"
            );
            let stores_line = java_stores
                .next()
                .unwrap_or_else(|| panic!("Java answered no stores ask for {dialect_name} {sha}"))
                .unwrap();
            let s: Vec<&str> = stores_line.split('\t').collect();
            assert!(
                s.len() == 3 && s[0] == sha && s[1] == dialect_name,
                "the stores oracle is out of step at {sha} {dialect_name}: {stores_line}"
            );
            let (form, stores) = model_form(&parsed, &boot);
            if sha256_utf16le(&form) == f[2] {
                matched += 1;
            } else {
                differ.push(format!("model {dialect_name} {sha}"));
                if let Some(dir) = &dump {
                    std::fs::write(
                        format!("{dir}/model.{dialect_name}.{sha}.rust.txt"),
                        String::from_utf16_lossy(&form),
                    )
                    .unwrap();
                }
            }
            if sha256_utf16le(&stores) == s[2] {
                s_matched += 1;
            } else {
                s_differ.push(format!("stores {dialect_name} {sha}"));
                if let Some(dir) = &dump {
                    std::fs::write(
                        format!("{dir}/stores.{dialect_name}.{sha}.rust.txt"),
                        String::from_utf16_lossy(&stores),
                    )
                    .unwrap();
                }
            }
        }
    }
    if limit == usize::MAX {
        assert!(
            java.next().is_none(),
            "the models oracle holds more lines than the asks"
        );
        assert!(
            java_stores.next().is_none(),
            "the stores oracle holds more lines than the asks"
        );
    }
    println!(
        "[names-differential] models: {asked} asks, {matched} identical to Java's, {} differ",
        differ.len()
    );
    println!(
        "[names-differential] stores: {asked} asks, {s_matched} identical to Java's, {} differ",
        s_differ.len()
    );
    differ.extend(s_differ);

    let boot_resolved =
        resolver::resolve(&lattice::boot_model().expect("the boot model")).expect("the boot layer resolves");
    let mut models: HashMap<String, Option<QueryModel>> = HashMap::new();
    let (mut pairs, mut q_matched) = (0usize, 0usize);
    let q_differ_start = differ.len();
    for line in BufReader::new(File::open(env("LITE_NAMES_QUERIES")).unwrap()).lines() {
        if pairs >= limit {
            break;
        }
        let line = line.unwrap();
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 3, "a line of the queries oracle is query, model, hash: {line}");
        pairs += 1;
        let model = models
            .entry(f[1].to_owned())
            .or_insert_with(|| query_model(&texts[f[1]], &boot, &boot_resolved));
        let form = query_form(&texts[f[0]], model.as_ref());
        if sha256_utf16le(&form) == f[2] {
            q_matched += 1;
        } else {
            differ.push(format!("query {} model {}", f[0], f[1]));
            if let Some(dir) = &dump {
                std::fs::write(
                    format!("{dir}/query.{}.{}.rust.txt", f[0], f[1]),
                    String::from_utf16_lossy(&form),
                )
                .unwrap();
            }
        }
    }
    println!(
        "[names-differential] queries: {pairs} pairs over {} models, {q_matched} identical to Java's, {} differ",
        models.len(),
        differ.len() - q_differ_start
    );

    let (mut shapes, mut c_matched) = (0usize, 0usize);
    let c_differ_start = differ.len();
    let mut by_builder: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    // every shape, whatever the limit: they are few and need no model
    for line in BufReader::new(File::open(env("LITE_NAMES_CALLS")).unwrap()).lines() {
        let line = line.unwrap();
        let f: Vec<&str> = line.split('\t').collect();
        assert!(f.len() >= 4, "a line of the calls oracle is a shape and a hash: {line}");
        shapes += 1;
        let (shape, hash) = (&f[..f.len() - 1], f[f.len() - 1]);
        *by_builder.entry(shape[0].to_owned()).or_default() += 1;
        let form = call_form(shape);
        if sha256_utf16le(&form) == hash {
            c_matched += 1;
        } else {
            differ.push(format!("call {}", shape.join(" ")));
            if let Some(dir) = &dump {
                std::fs::write(
                    format!("{dir}/call.{}.rust.txt", differ.len()),
                    format!("{}\n{}", shape.join("\t"), String::from_utf16_lossy(&form)),
                )
                .unwrap();
            }
        }
    }
    println!(
        "[names-differential] calls: {shapes} shapes {by_builder:?}, {c_matched} identical to Java's, {} differ",
        differ.len() - c_differ_start
    );
    for d in differ.iter().take(SHOWN) {
        println!("[names-differential]   differs: {d}");
    }
    assert!(differ.is_empty(), "{} names answers differ from Java's", differ.len());
}
