// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `EngineHandlers`: the engine's bare-name function surface (core's `engine-handlers.tsv`, written by
//! //spec:gen_engine_handlers from the engine's `Handlers.java` and read here as the same committed file), each name
//! with its signature ids, joined by id to the platform's declarations (the catalog's natives, then the prelude's
//! functions), and the platform's own surface (`Pure.liteSurfaceFunctions`) joined the same way.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::pure::{NativeId, lite_surface_functions};

/// The table's text, the committed file the Java reads.
pub const TABLE: &str = include_str!("../../../core/src/main/resources/com/legend/builtin/engine-handlers.tsv");

pub struct EngineHandlers {
    /// name -> the FQNs the platform declares for the engine's ids under that name, in first-joined order
    fqns: Vec<(String, Vec<String>)>,
    /// name -> every engine id under that name, declared here or not
    ids: Vec<(String, Vec<String>)>,
    fqns_index: HashMap<String, usize>,
    ids_index: HashMap<String, usize>,
    undeclared: Vec<String>,
    unmatched_surface: Vec<String>,
}

static HANDLERS: LazyLock<EngineHandlers> = LazyLock::new(build);

/// The surface (joined on first read).
pub fn handlers() -> &'static EngineHandlers {
    &HANDLERS
}

fn build() -> EngineHandlers {
    // the platform's declaration of each signature id: the catalog's natives, then the prelude's functions
    let mut declared: HashMap<String, String> = HashMap::new();
    for n in NativeId::all() {
        // a later native of the same id replaces the earlier, as LinkedHashMap.put does
        declared.insert(n.id().qualified.lossy(), n.def().qualified_name.lossy());
    }
    for el in crate::prelude::prelude().elements() {
        // every function the module declares, bodied or native (Java's `instanceof Function`)
        if let Some(f) = el.as_function() {
            let id = lite_model::function_id::mangle(&f).lossy();
            declared.entry(id).or_insert_with(|| f.qualified_name().lossy());
        }
    }
    let mut h = EngineHandlers {
        fqns: Vec::new(),
        ids: Vec::new(),
        fqns_index: HashMap::new(),
        ids_index: HashMap::new(),
        undeclared: Vec::new(),
        unmatched_surface: Vec::new(),
    };
    for line in TABLE.split('\n') {
        if line.trim().is_empty() || line.starts_with('#') || line == "name\tid" {
            continue;
        }
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() != 2 {
            panic!("engine-handlers.tsv: expected 2 columns (name, id): {line}");
        }
        let fqn = declared.get(c[1]).cloned();
        if fqn.is_none() {
            h.undeclared.push(c[1].to_owned());
        }
        h.add(c[0], c[1], fqn);
    }
    // the platform's own surface, joined like the engine's rows, its names in order
    let mut surface = lite_surface_functions();
    surface.sort_by_key(|(name, _)| *name);
    for (name, group) in surface {
        for n in group {
            let id = n.id().qualified.lossy();
            match declared.get(&id).cloned() {
                None => h.unmatched_surface.push(id),
                Some(fqn) => h.add(name, &id, Some(fqn)),
            }
        }
    }
    h
}

impl EngineHandlers {
    fn add(&mut self, name: &str, id: &str, fqn: Option<String>) {
        let i = *self.ids_index.entry(name.to_owned()).or_insert_with(|| {
            self.ids.push((name.to_owned(), Vec::new()));
            self.ids.len() - 1
        });
        self.ids[i].1.push(id.to_owned());
        if let Some(fqn) = fqn {
            let j = *self.fqns_index.entry(name.to_owned()).or_insert_with(|| {
                self.fqns.push((name.to_owned(), Vec::new()));
                self.fqns.len() - 1
            });
            if !self.fqns[j].1.contains(&fqn) {
                self.fqns[j].1.push(fqn);
            }
        }
    }

    /// `EngineHandlers.fqnsOf(name)`: the FQNs a bare name denotes on the engine surface (empty: not a handler name).
    pub fn fqns_of(&self, name: &str) -> &[String] {
        self.fqns_index.get(name).map_or(&[], |&i| self.fqns[i].1.as_slice())
    }

    /// `EngineHandlers.idsOf(name)`: the engine's signature ids under a bare name, declared here or not.
    pub fn ids_of(&self, name: &str) -> &[String] {
        self.ids_index.get(name).map_or(&[], |&i| self.ids[i].1.as_slice())
    }

    /// `EngineHandlers.names()`: every bare name the engine registers a handler for, in first-seen order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.ids.iter().map(|(n, _)| n.as_str())
    }

    /// `EngineHandlers.undeclaredIds()`: the engine's ids the platform declares nowhere.
    pub fn undeclared_ids(&self) -> &[String] {
        &self.undeclared
    }

    /// `EngineHandlers.unmatchedSurface()`: the platform's surface ids no declaration carries.
    pub fn unmatched_surface(&self) -> &[String] {
        &self.unmatched_surface
    }
}
