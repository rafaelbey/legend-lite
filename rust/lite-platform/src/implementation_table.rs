// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `ImplementationTable`: what executes every declaration -- one [`Implementation`] per `FunctionId` of a
//! [`DeclarationTable`], total by construction.
//!
//! Built only from explicit [`Registrations`], each read exactly: a lowering key is matched to the declaration whose
//! id it is (whole, never parsed); a family member's overloads are definitions; a form, a wall, a walled body and a
//! subsumed program name exact FQNs, and apply to every declaration there. A declaration no registration names takes
//! the default of its kind -- `Unimplemented` when it is a native; when it has a body, `Body`, unless the catalog
//! declares a function at its FQN or the platform implements one there in its own Pure: then it is a version of a
//! function the platform declares, with no row of its own, and is refused (`NO_ROW`) unless a decision names its id.
//!
//! A registration that names nothing the table declares is dangling; two registrations that cannot both hold are
//! conflicts. Both are reported, never resolved silently.
//!
//! Ported from core's `com.legend.platform.ImplementationTable` (2026-11-12, docs/rust-port/PHASE3.md slice 1b): the
//! rows in declaration order, the reports in the order the registrations are read, each message Java's text -- a set
//! as Java's `toString` writes it (`[SCALAR, AGGREGATE]`; a family as its class, `class
//! com.legend.builtin.NativeFn$Verdict`). The builder's maps keyed by id are asked by id only, never iterated, so they
//! are hash maps here; a set an id collects keeps Java's order (an `EnumSet`'s enum order, a `LinkedHashSet`'s first
//! one).

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use lite_builtin::native_fn::Family;
use lite_model::{ClassMember, Function, FunctionId, JStr, SynthHat};

use crate::core_fn::CoreFn;
use crate::declaration_table::DeclarationTable;
use crate::feature::Feature;
use crate::implementation::{Form, Implementation, Intrinsic, Position, Reason, Refused};
use crate::registrations::Registrations;
use crate::walled_bodies::{Kind, Wall};

/// What executes every declaration of a table.
#[derive(Clone, Debug)]
pub struct ImplementationTable {
    rows: Vec<(FunctionId, Implementation)>,
    index: HashMap<FunctionId, usize>,
    dangling: Vec<JStr>,
    conflicts: Vec<JStr>,
    member_walls: Vec<JStr>,
}

static BODY: Implementation = Implementation::Body;
static UNIMPLEMENTED: Implementation = Implementation::Unimplemented;

/// The members of a list Java holds as a set, each once, at its first place.
fn as_set<T: Eq + Hash>(values: &[T]) -> impl Iterator<Item = &T> {
    let mut seen = HashSet::new();
    values.iter().filter(move |v| seen.insert(*v))
}

/// An `EnumSet`'s `add`: the members kept in their enum's order.
fn enum_set_add<T: Ord + Copy>(set: &mut Vec<T>, v: T) {
    if let Err(at) = set.binary_search(&v) {
        set.insert(at, v);
    }
}

/// A `LinkedHashSet`'s `add`: a new member appended.
fn linked_set_add<T: PartialEq>(set: &mut Vec<T>, v: T) {
    if !set.contains(&v) {
        set.push(v);
    }
}

/// A family as Java's `Class.toString` writes it.
fn family_class(f: Family) -> String {
    format!("class com.legend.builtin.NativeFn${}", f.name())
}

/// A set as Java's `AbstractCollection.toString` writes it: `[a, b]`.
fn set_text(members: impl Iterator<Item = String>) -> String {
    format!("[{}]", members.collect::<Vec<_>>().join(", "))
}

fn positions_text(ps: &[Position]) -> String {
    set_text(ps.iter().map(|p| p.name().to_owned()))
}

fn families_text(fs: &[Family]) -> String {
    set_text(fs.iter().map(|&f| family_class(f)))
}

fn reason_of(kind: Kind) -> Reason {
    match kind {
        Kind::ENGINE_MACHINERY => Reason::ENGINE_MACHINERY,
        Kind::CANNOT_IMPLEMENT => Reason::CANNOT_IMPLEMENT,
    }
}

fn refused_of(wall: &Wall) -> Refused {
    Refused {
        reason: reason_of(wall.kind),
        why: wall.why.clone(),
    }
}

/// Every declaration at `fqn` refused, or the registration dangling when there is none.
fn refuse(
    fqn: &JStr,
    refusal: &Refused,
    source: &str,
    declarations: &DeclarationTable,
    refused: &mut HashMap<FunctionId, Refused>,
    dangling: &mut Vec<JStr>,
) {
    let at = declarations.at(fqn);
    if at.is_empty() {
        dangling.push(cat!(source, " ", fqn));
    }
    for f in at {
        refused.insert(FunctionId::of(f), refusal.clone());
    }
}

impl ImplementationTable {
    /// `ImplementationTable.build(declarations, registrations)`: the table over `declarations`, from `registrations`.
    pub fn build(declarations: &DeclarationTable, registrations: &Registrations) -> ImplementationTable {
        let mut dangling: Vec<JStr> = Vec::new();
        let mut conflicts: Vec<JStr> = Vec::new();
        let mut member_walls: Vec<JStr> = Vec::new();

        // the lowering registers by identity: a key is a declaration's id, matched whole against the declarations
        let mut positions: HashMap<FunctionId, Vec<Position>> = HashMap::new();
        for (position, ids) in registrations.lowering_keys.iter() {
            for id in as_set(ids) {
                if declarations.get(id).is_none() {
                    dangling.push(cat!(position.name(), " ", &id.qualified));
                    continue;
                }
                enum_set_add(positions.entry(id.clone()).or_default(), *position);
            }
        }
        let mut overrides: HashMap<FunctionId, Vec<Feature>> = HashMap::new();
        for (feature, ids) in registrations.feature_overrides.iter() {
            for id in as_set(ids) {
                if declarations.get(id).is_none() {
                    dangling.push(cat!("feature ", feature.name(), " ", &id.qualified));
                    continue;
                }
                enum_set_add(overrides.entry(id.clone()).or_default(), *feature);
            }
        }

        // the implementer families: each member's overloads are definitions
        let mut families: HashMap<FunctionId, Vec<Family>> = HashMap::new();
        for (family, overloads) in registrations.families.iter() {
            for o in overloads {
                let id = FunctionId::of(&Function::NativeFunctionDefinition(o.clone()));
                if declarations.get(&id).is_none() {
                    dangling.push(cat!(family.name(), " ", &id.qualified));
                    continue;
                }
                linked_set_add(families.entry(id).or_default(), *family);
            }
        }

        // the class members a family implements: a lifted derived-property declaration whose provenance names a
        // registered member
        for (id, f) in declarations.entries() {
            if let Function::FunctionDefinition(fd) = f
                && let Some(s) = &fd.synthesized_from
                && s.hat == SynthHat::PROP
            {
                let member = ClassMember {
                    owner: s.owner_fqn.clone(),
                    name: s.member_name.clone(),
                };
                for (family, members) in registrations.members.iter() {
                    if members.contains(&member) {
                        linked_set_add(families.entry(id.clone()).or_default(), *family);
                    }
                }
            }
        }

        // the language forms: every overload at each FQN a form owns
        let mut forms: HashMap<FunctionId, CoreFn> = HashMap::new();
        for (form, fqns) in registrations.forms.iter() {
            for fqn in as_set(fqns) {
                let at = declarations.at(fqn);
                if at.is_empty() {
                    dangling.push(cat!("CoreFn.", form.name(), " ", fqn));
                }
                for f in at {
                    let id = FunctionId::of(f);
                    if let Some(prior) = forms.insert(id.clone(), *form)
                        && prior != *form
                    {
                        conflicts.push(cat!(&id.qualified, ": forms ", prior.name(), " and ", form.name()));
                    }
                }
            }
        }

        // the refusals, each with its reason
        let mut refused: HashMap<FunctionId, Refused> = HashMap::new();
        for (fqn, why) in registrations.walled_natives.iter() {
            let refusal = Refused {
                reason: Reason::CANNOT_IMPLEMENT,
                why: why.clone(),
            };
            refuse(
                fqn,
                &refusal,
                "walled native",
                declarations,
                &mut refused,
                &mut dangling,
            );
        }
        for (fqn, wall) in registrations.walled_bodies.iter() {
            if declarations.at(fqn).is_empty() {
                // a walled class member body (a lifted derived property or constraint), not a function: apart
                member_walls.push(fqn.clone());
                continue;
            }
            refuse(
                fqn,
                &refused_of(wall),
                "walled body",
                declarations,
                &mut refused,
                &mut dangling,
            );
        }
        let moot = Refused {
            reason: Reason::MOOT,
            why: JStr::from("subsumed: its value is never needed on this platform"),
        };
        for fqn in as_set(&registrations.subsumed) {
            refuse(fqn, &moot, "subsumed", declarations, &mut refused, &mut dangling);
        }

        // the FQNs the catalog declares a function at: a bodied declaration there that no registration names is a
        // version of a function the platform declares
        let mut catalog_fqns: HashSet<JStr> = registrations.catalog.iter().map(|n| n.qualified_name.clone()).collect();
        // the platform's own Pure, by id: the same rule covers the names it implements at
        let mut platform_pure: HashSet<FunctionId> = HashSet::new();
        for id in as_set(&registrations.platform_pure) {
            match declarations.get(id) {
                None => dangling.push(cat!("platform's own Pure ", &id.qualified)),
                Some(f) => {
                    platform_pure.insert(id.clone());
                    catalog_fqns.insert(f.qualified_name().clone());
                }
            }
        }
        // the decisions on the other versions at those names, by id
        let mut upstream_bodies: HashSet<FunctionId> = HashSet::new();
        for (id, _) in registrations.upstream_bodies.iter() {
            if !matches!(declarations.get(id), Some(Function::FunctionDefinition(_))) {
                dangling.push(cat!("upstream body ", &id.qualified));
                continue;
            }
            upstream_bodies.insert(id.clone());
        }
        for (id, wall) in registrations.refused_versions.iter() {
            if declarations.get(id).is_none() {
                dangling.push(cat!("refused version ", &id.qualified));
                continue;
            }
            refused.insert(id.clone(), refused_of(wall));
        }

        // one row per declaration
        let none: Vec<Position> = Vec::new();
        let no_features: Vec<Feature> = Vec::new();
        let no_families: Vec<Family> = Vec::new();
        let mut rows: Vec<(FunctionId, Implementation)> = Vec::with_capacity(declarations.size());
        for (id, declaration) in declarations.entries() {
            let ps = positions.get(id).unwrap_or(&none);
            let fo = overrides.get(id).unwrap_or(&no_features);
            let fs = families.get(id).unwrap_or(&no_families);
            let form = forms.get(id).copied();
            let refusal = refused.get(id);
            let implemented = !ps.is_empty() || !fs.is_empty();
            let own_pure = platform_pure.contains(id);
            let decided_body = upstream_bodies.contains(id);
            let other = || {
                if own_pure {
                    "the platform's own Pure"
                } else {
                    "an upstream body"
                }
            };
            let row = if let Some(form) = form {
                if let Some(refusal) = refusal {
                    conflicts.push(cat!(
                        &id.qualified,
                        ": form ",
                        form.name(),
                        " and a refusal (",
                        refusal.reason.name(),
                        ")"
                    ));
                }
                if own_pure || decided_body {
                    conflicts.push(cat!(&id.qualified, ": form ", form.name(), " and ", other()));
                }
                Implementation::Form(Form::new(form, ps, fs))
            } else if let Some(refusal) = refusal {
                if implemented {
                    conflicts.push(cat!(
                        &id.qualified,
                        ": refused (",
                        refusal.reason.name(),
                        ") and implemented ",
                        positions_text(ps),
                        " ",
                        families_text(fs)
                    ));
                }
                if own_pure || decided_body {
                    conflicts.push(cat!(
                        &id.qualified,
                        ": refused (",
                        refusal.reason.name(),
                        ") and ",
                        other()
                    ));
                }
                Implementation::Refused(refusal.clone())
            } else if own_pure {
                if implemented {
                    conflicts.push(cat!(
                        &id.qualified,
                        ": the platform's own Pure and implemented ",
                        positions_text(ps),
                        " ",
                        families_text(fs)
                    ));
                } else if decided_body {
                    conflicts.push(cat!(&id.qualified, ": the platform's own Pure and an upstream body"));
                }
                Implementation::PlatformPure
            } else if implemented {
                if decided_body {
                    conflicts.push(cat!(
                        &id.qualified,
                        ": implemented ",
                        positions_text(ps),
                        " ",
                        families_text(fs),
                        " and an upstream body"
                    ));
                }
                Implementation::Intrinsic(
                    Intrinsic::new(ps, fo, fs).expect("an implemented declaration has a position or a family"),
                )
            } else if decided_body {
                Implementation::Body
            } else if let Function::FunctionDefinition(fd) = declaration {
                if catalog_fqns.contains(&fd.qualified_name) {
                    Implementation::Refused(Refused {
                        reason: Reason::NO_ROW,
                        why: cat!(
                            "the version ",
                            &id.qualified,
                            " of a function the platform declares has no row of its own (upstream's body is the",
                            " spec, not this platform's implementation)"
                        ),
                    })
                } else {
                    Implementation::Body
                }
            } else {
                Implementation::Unimplemented
            };
            rows.push((id.clone(), row));
        }
        let index = rows.iter().enumerate().map(|(i, (id, _))| (id.clone(), i)).collect();
        ImplementationTable {
            rows,
            index,
            dangling,
            conflicts,
            member_walls,
        }
    }

    /// `of(id)`: the implementation of the declaration `id`, if the table declares it.
    pub fn of(&self, id: &FunctionId) -> Option<&Implementation> {
        self.index.get(id).map(|&i| &self.rows[i].1)
    }

    /// `rowOf(declaration)`: the row for `declaration`, or -- for one outside this table -- the default of its kind:
    /// `Body` for a body, `Unimplemented` for a native. A lowering with no model behind it meets such declarations.
    pub fn row_of(&self, declaration: &Function) -> &Implementation {
        match self.of(&FunctionId::of(declaration)) {
            Some(row) => row,
            None => match declaration {
                Function::FunctionDefinition(_) => &BODY,
                Function::NativeFunctionDefinition(_) => &UNIMPLEMENTED,
            },
        }
    }

    /// `runsByRule(declaration)`: whether the platform runs `declaration` by its own rule or form (never by the
    /// declaration's body); false for none or an undeclared one.
    pub fn runs_by_rule(&self, declaration: Option<&Function>) -> bool {
        declaration.is_some_and(|d| Implementation::by_rule(self.of(&FunctionId::of(d))))
    }

    /// `rows()`: every row, in declaration order.
    pub fn rows(&self) -> &[(FunctionId, Implementation)] {
        &self.rows
    }

    /// `dangling()`: the registrations naming nothing the table declares.
    pub fn dangling(&self) -> &[JStr] {
        &self.dangling
    }

    /// `conflicts()`: the registrations that cannot both hold.
    pub fn conflicts(&self) -> &[JStr] {
        &self.conflicts
    }

    /// `memberWalls()`: the walled class-member bodies (lifted derived properties, constraints): walls that are not
    /// function rows.
    pub fn member_walls(&self) -> &[JStr] {
        &self.member_walls
    }
}

/// ImplementationTableTest: every path of the table builder, over hand-written registrations.
#[cfg(test)]
mod tests {
    use lite_model::linked::LinkedMap;

    use super::*;
    use crate::testing::{LOWER_ID, UPPER_ID, catalog, declare, fqn, id, native, on_stack};

    fn table(declared: Vec<Function>, registrations: &Registrations) -> ImplementationTable {
        ImplementationTable::build(&DeclarationTable::of(declared), registrations)
    }

    fn texts(values: &[JStr]) -> Vec<String> {
        values.iter().map(JStr::lossy).collect()
    }

    #[test]
    fn defaults_are_body_for_a_body_and_unimplemented_for_a_native() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let mut ds = vec![native(&upper)];
            ds.extend(declare("function my::pkg::twice(x:Integer[1]):Integer[1] { $x }"));
            let t = table(ds, &Registrations::of_catalog(vec![upper]));
            assert_eq!(t.rows().len(), 2);
            assert_eq!(t.of(&id(UPPER_ID)), Some(&Implementation::Unimplemented));
            assert_eq!(
                t.of(&id("my::pkg::twice_Integer_1__Integer_1_")),
                Some(&Implementation::Body)
            );
            assert!(t.dangling().is_empty());
            assert!(t.conflicts().is_empty());
        });
    }

    #[test]
    fn a_lowering_key_is_matched_to_its_catalog_definition_whole() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                lowering_keys: LinkedMap::of([(Position::SCALAR, vec![FunctionId::of(&native(&upper))])]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            let Some(Implementation::Intrinsic(row)) = t.of(&id(UPPER_ID)) else {
                panic!("an intrinsic")
            };
            assert_eq!(row.positions(), &[Position::SCALAR]);
        });
    }

    #[test]
    fn a_key_no_declaration_has_is_dangling() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                lowering_keys: LinkedMap::of([(Position::SCALAR, vec![id("no::such::function_String_1__String_1_")])]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            assert_eq!(t.dangling().len(), 1);
            assert!(t.dangling()[0].lossy().contains("no::such::function"));
        });
    }

    #[test]
    fn a_family_member_and_a_form_are_read_from_their_definitions_and_fqns() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let lower = catalog(LOWER_ID);
            let r = Registrations {
                families: LinkedMap::of([(Family::Verdict, vec![upper.clone()])]),
                forms: LinkedMap::of([(CoreFn::FILTER, vec![fqn(&lower)])]),
                ..Registrations::of_catalog(vec![upper.clone(), lower.clone()])
            };
            let t = table(vec![native(&upper), native(&lower)], &r);
            let Some(Implementation::Intrinsic(u)) = t.of(&id(UPPER_ID)) else {
                panic!("an intrinsic")
            };
            assert_eq!(u.families(), &[Family::Verdict]);
            let Some(Implementation::Form(l)) = t.of(&id(LOWER_ID)) else {
                panic!("a form")
            };
            assert_eq!(l.form(), CoreFn::FILTER);
            assert!(t.conflicts().is_empty());
        });
    }

    #[test]
    fn a_form_that_also_has_a_rule_keeps_it_as_its_delegate() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                lowering_keys: LinkedMap::of([(Position::SCALAR, vec![FunctionId::of(&native(&upper))])]),
                forms: LinkedMap::of([(CoreFn::MAP, vec![fqn(&upper)])]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            let Some(Implementation::Form(f)) = t.of(&id(UPPER_ID)) else {
                panic!("a form")
            };
            assert_eq!(f.also_lowered(), &[Position::SCALAR]);
            assert!(t.conflicts().is_empty());
        });
    }

    #[test]
    fn a_refusal_beside_an_implementation_is_a_conflict() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                lowering_keys: LinkedMap::of([(Position::SCALAR, vec![FunctionId::of(&native(&upper))])]),
                walled_natives: LinkedMap::of([(fqn(&upper), JStr::from("an effect"))]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            assert_eq!(
                texts(t.conflicts()),
                [format!(
                    "{UPPER_ID}: refused (CANNOT_IMPLEMENT) and implemented [SCALAR] []"
                )]
            );
            assert!(matches!(t.of(&id(UPPER_ID)), Some(Implementation::Refused(_))));
        });
    }

    #[test]
    fn two_forms_on_one_declaration_is_a_conflict() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                forms: LinkedMap::of([(CoreFn::MAP, vec![fqn(&upper)]), (CoreFn::FILTER, vec![fqn(&upper)])]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            assert_eq!(texts(t.conflicts()), [format!("{UPPER_ID}: forms MAP and FILTER")]);
            // the later form is the row's, as Java's map put leaves it
            assert!(matches!(t.of(&id(UPPER_ID)), Some(Implementation::Form(f)) if f.form() == CoreFn::FILTER));
        });
    }

    #[test]
    fn refusals_carry_their_kind_and_walled_members_are_reported_apart() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let mut ds = vec![native(&upper)];
            ds.extend(declare("function my::pkg::printer(x:Integer[1]):Integer[1] { $x }"));
            let r = Registrations {
                walled_natives: LinkedMap::of([(fqn(&upper), JStr::from("an effect"))]),
                walled_bodies: LinkedMap::of([
                    (
                        JStr::from("my::pkg::printer"),
                        Wall::new(Kind::ENGINE_MACHINERY, "the printer"),
                    ),
                    (
                        JStr::from("my::pkg::Cls$prop$derived"),
                        Wall::new(Kind::CANNOT_IMPLEMENT, "x"),
                    ),
                ]),
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(ds, &r);
            let Some(Implementation::Refused(walled_native)) = t.of(&id(UPPER_ID)) else {
                panic!("refused")
            };
            assert_eq!(walled_native.reason, Reason::CANNOT_IMPLEMENT);
            let Some(Implementation::Refused(body)) = t.of(&id("my::pkg::printer_Integer_1__Integer_1_")) else {
                panic!("refused")
            };
            assert_eq!(body.reason, Reason::ENGINE_MACHINERY);
            assert_eq!(texts(t.member_walls()), ["my::pkg::Cls$prop$derived"]);
            assert!(t.dangling().is_empty());
        });
    }

    #[test]
    fn a_subsumed_fqn_no_declaration_has_is_dangling() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let r = Registrations {
                subsumed: vec![JStr::from("no::such::program")],
                ..Registrations::of_catalog(vec![upper.clone()])
            };
            let t = table(vec![native(&upper)], &r);
            assert_eq!(texts(t.dangling()), ["subsumed no::such::program"]);
        });
    }

    #[test]
    fn a_declaration_outside_the_table_takes_the_default_of_its_kind() {
        on_stack(|| {
            let upper = catalog(UPPER_ID);
            let t = table(vec![], &Registrations::of_catalog(vec![]));
            let body = declare("function my::pkg::outside(x:Integer[1]):Integer[1] { $x }");
            assert_eq!(t.row_of(&body[0]), &Implementation::Body);
            assert_eq!(t.row_of(&native(&upper)), &Implementation::Unimplemented);
            assert!(!t.runs_by_rule(Some(&body[0])));
            assert!(!t.runs_by_rule(None));
        });
    }
}
