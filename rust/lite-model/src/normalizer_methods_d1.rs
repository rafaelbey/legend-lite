// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Model methods AssociationSynthesis and ViewRelation read that the records do not carry yet (2026-11-14, the
//! normalizer slice's fill, helper d1):
//!
//! - `FilterPointer.name()` (FilterPointer.java:35): every variant's filter name.
//!
//! (`AssociationDefinition.AssociationEndDefinition.targetClassFqn()` is helper a's, `normalizer_methods_a.rs`.)

use lite_front::jtypes::JStr;

use crate::records::FilterPointer;

impl FilterPointer {
    /// `name()`: the filter's name, on every variant.
    pub fn name(&self) -> &JStr {
        match self {
            FilterPointer::FilterPointerLocal(l) => &l.name,
            FilterPointer::FilterPointerCross(c) => &c.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::records::{FilterPointerCross, FilterPointerLocal};
    use crate::text::j;

    #[test]
    fn a_filter_pointer_names_its_filter() {
        let local = FilterPointer::FilterPointerLocal(Arc::new(FilterPointerLocal { name: j("F") }));
        let cross = FilterPointer::FilterPointerCross(Arc::new(FilterPointerCross {
            db: j("a::db"),
            name: j("G"),
        }));
        assert_eq!((local.name(), cross.name()), (&j("F"), &j("G")));
    }
}
