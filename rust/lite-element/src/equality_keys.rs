// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `EqualityKeys.isKeyStereotype` (EqualityKeys.java:74 at 18feff3fe): the one spelling of the `<<equality.Key>>`
//! test, by which the class compiler stamps `PropertyStored::equality_key`. The rest of `EqualityKeys` (`fqnOf`,
//! `resolve`, `declaresKeys`, the key trees) is I's and testplan's (EF_CENSUS §3, ruling 27), ported with them.

use lite_model::StereotypeApplication;
use lite_types::platform::{EQUALITY_PROFILE, is_profile};

/// `isKeyStereotype(st)` (:74): the stereotype is `Key` of the platform's `meta::pure::profiles::equality`.
pub fn is_key_stereotype(st: &StereotypeApplication) -> bool {
    // the profile's name is the resolver's (resolved through the imports), so `is_profile` is an equality; Rust text
    // of a name holding a lone surrogate (U+FFFD for it) equals no platform FQN, as its units do not
    is_profile(&st.profile_name.lossy(), EQUALITY_PROFILE) && st.stereotype_name.eq_str("Key")
}

#[cfg(test)]
mod tests {
    use lite_front::doors::Dialect;
    use lite_model::text::j;
    use lite_model::{PackageableElement, StereotypeApplication};

    use super::is_key_stereotype;

    fn st(profile: &str, stereotype: &str) -> StereotypeApplication {
        StereotypeApplication {
            profile_name: j(profile),
            stereotype_name: j(stereotype),
        }
    }

    #[test]
    fn the_key_stereotype_is_the_platforms_equality_key() {
        assert!(is_key_stereotype(&st("meta::pure::profiles::equality", "Key")));
        // a bare spelling is a profile the model has not (the resolver qualifies the platform's): never the platform's
        assert!(!is_key_stereotype(&st("equality", "Key")));
        assert!(!is_key_stereotype(&st("x::equality", "Key")));
        assert!(!is_key_stereotype(&st("meta::pure::profiles::equality", "key")));
        assert!(!is_key_stereotype(&st("meta::pure::profiles::temporal", "Key")));
    }

    #[test]
    fn a_declared_key_is_found_on_the_resolved_model() {
        // the resolver qualifies `equality` through the core imports, as the class compiler meets it
        let text = "###Pure\nClass x::A\n{\n  <<equality.Key>> id: Integer[1];\n  name: String[1];\n}\n";
        let units: Vec<u16> = text.encode_utf16().collect();
        let parsed = lite_builtin::door::parse(&units, Dialect::LegendLite).expect("the model parses");
        let resolved = lite_names::resolver::resolve(&parsed).expect("the model's names resolve");
        let cd = resolved
            .elements
            .iter()
            .find_map(|e| match e {
                PackageableElement::ClassDefinition(c) => Some(c.clone()),
                _ => None,
            })
            .expect("the class");
        let keyed: Vec<bool> = cd
            .properties
            .iter()
            .map(|pd| pd.stereotypes.iter().any(|s| is_key_stereotype(s)))
            .collect();
        assert_eq!(keyed, vec![true, false]);
    }
}
