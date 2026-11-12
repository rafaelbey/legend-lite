// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ParsedModel` (step B's result: the elements, the import scope, and the side maps keyed by an element's key) and
//! `ImportScope` (a source's wildcard imports, built once by its `Builder`).

use std::sync::Arc;

use lite_front::jtypes::JStr;

use crate::error::{Thrown, illegal_argument};
use crate::linked::{empty, get, list};
use crate::records::{
    Function, FunctionId, ImportScope, PackageableElement, ParsedModel, ParsedModelPosition,
    ParsedModelUnclaimedSection,
};

impl ImportScope {
    /// `ImportScope.empty()`.
    pub fn empty() -> Arc<ImportScope> {
        Arc::new(ImportScope { wildcards: empty() })
    }

    /// `isEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.wildcards.is_empty()
    }
}

/// `ImportScope.Builder`: the imports of a source as they are read, each package once, in order.
#[derive(Clone, Debug, Default)]
pub struct ImportScopeBuilder {
    wildcards: Vec<JStr>,
}

impl ImportScopeBuilder {
    pub fn new() -> ImportScopeBuilder {
        ImportScopeBuilder::default()
    }

    /// `add(importStatement)`: a package wildcard (`a::b::*`), anything else refused loudly.
    pub fn add(&mut self, import_statement: &JStr) -> Result<&mut ImportScopeBuilder, Thrown> {
        let path = java_trim(&import_statement.0);
        let star: Vec<u16> = "::*".encode_utf16().collect();
        if !path.ends_with(&star) || path.len() == 3 {
            return Err(illegal_argument(crate::cat!(
                "malformed import '",
                path,
                "': an import must be a package wildcard (a::b::*)"
            )));
        }
        let pkg = JStr(path[..path.len() - 3].to_vec());
        if !self.wildcards.contains(&pkg) {
            self.wildcards.push(pkg);
        }
        Ok(self)
    }

    /// `build()`.
    pub fn build(&self) -> Arc<ImportScope> {
        Arc::new(ImportScope {
            wildcards: list(self.wildcards.clone()),
        })
    }
}

/// `String.trim()`: every leading and trailing unit up to U+0020.
fn java_trim(s: &[u16]) -> &[u16] {
    let mut start = 0;
    let mut end = s.len();
    while start < end && s[start] <= 0x20 {
        start += 1;
    }
    while end > start && s[end - 1] <= 0x20 {
        end -= 1;
    }
    &s[start..end]
}

impl ParsedModel {
    /// The canonical constructor's checks: one kind of position per model -- offsets into a text, or the spans its
    /// records carried, never both.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        elements: Arc<[PackageableElement]>,
        imports: Arc<ImportScope>,
        source: Option<JStr>,
        element_offsets: Arc<[(JStr, i32)]>,
        element_imports: Arc<[(JStr, Arc<ImportScope>)]>,
        element_sources: Arc<[(JStr, JStr)]>,
        unclaimed_sections: Arc<[Arc<ParsedModelUnclaimedSection>]>,
        element_spans: Arc<[(JStr, Arc<ParsedModelPosition>)]>,
    ) -> Result<ParsedModel, Thrown> {
        if !element_spans.is_empty() && !element_offsets.is_empty() {
            return Err(illegal_argument(JStr::from(
                "a model has offsets into a text or spans from its records, not both",
            )));
        }
        Ok(ParsedModel {
            elements,
            imports,
            source,
            element_offsets,
            element_imports,
            element_sources,
            unclaimed_sections,
            element_spans,
        })
    }

    /// `isEmpty()`: no elements and no imports.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.imports.is_empty()
    }

    /// `keyOf(el)`: a function's id, any other element's qualified name.
    pub fn key_of(el: &PackageableElement) -> JStr {
        match el.as_function() {
            Some(f) => FunctionId::of(&f).qualified,
            None => el.qualified_name().clone(),
        }
    }

    /// `keyOf` of a function.
    pub fn key_of_function(f: &Function) -> JStr {
        FunctionId::of(f).qualified
    }

    /// `position(key)`: `"[line:col]"` from the element's span, or from its offset into the source; `None` when the
    /// model does not say.
    pub fn position(&self, key: &JStr) -> Option<String> {
        if let Some(span) = get(&self.element_spans, key) {
            return Some(format!("[{}:{}]", span.line, span.column));
        }
        match (get(&self.element_offsets, key), &self.source) {
            (Some(&offset), Some(source)) => Some(position_in(source, offset)),
            _ => None,
        }
    }
}

/// `ParsedModel.positionIn(text, offset)`: `"[line:col]"` of unit `offset` in `text`, both from 1.
pub fn position_in(text: &JStr, offset: i32) -> String {
    let mut line = 1;
    let mut col = 1;
    let end = (offset.max(0) as usize).min(text.len());
    for &u in &text.0[..end] {
        if u == u16::from(b'\n') {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    format!("[{line}:{col}]")
}
