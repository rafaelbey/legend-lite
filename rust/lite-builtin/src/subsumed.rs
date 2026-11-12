// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! `Subsumed` (the platform's decisions on engine programs whose value no code here needs) and `TdsLegacy` (the legacy
//! TDS vocabulary the typer recognizes by its spelled name).

use lite_front::records as p;

/// `Subsumed`: an engine program whose value is never needed here, with where upstream defines it and why.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Subsumed {
    CREATE_DB_CONFIG,
    RELATIONAL_EXTENSIONS,
}

impl Subsumed {
    /// Java's `values()`.
    pub const VALUES: [Subsumed; 2] = [Subsumed::CREATE_DB_CONFIG, Subsumed::RELATIONAL_EXTENSIONS];

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        match self {
            Subsumed::CREATE_DB_CONFIG => "CREATE_DB_CONFIG",
            Subsumed::RELATIONAL_EXTENSIONS => "RELATIONAL_EXTENSIONS",
        }
    }

    pub fn fqn(self) -> &'static str {
        match self {
            Subsumed::CREATE_DB_CONFIG => "meta::relational::functions::sqlQueryToString::createDbConfig",
            Subsumed::RELATIONAL_EXTENSIONS => "meta::relational::extension::relationalExtensions",
        }
    }

    pub fn engine_file(self) -> &'static str {
        match self {
            Subsumed::CREATE_DB_CONFIG => concat!(
                "legend-engine-xts-relationalStore/legend-engine-xt-relationalStore-generation/",
                "legend-engine-xt-relationalStore-pure/legend-engine-xt-relationalStore-core-pure/",
                "src/main/resources/core_relational/relational/sqlQueryToString/dbExtension.pure"
            ),
            Subsumed::RELATIONAL_EXTENSIONS => concat!(
                "legend-engine-xts-relationalStore/legend-engine-xt-relationalStore-generation/",
                "legend-engine-xt-relationalStore-pure/legend-engine-xt-relationalStore-core-pure/",
                "src/main/resources/core_relational/relational/extensions/extension.pure"
            ),
        }
    }

    pub fn from_line(self) -> i32 {
        match self {
            Subsumed::CREATE_DB_CONFIG => 241,
            Subsumed::RELATIONAL_EXTENSIONS => 62,
        }
    }

    pub fn to_line(self) -> i32 {
        match self {
            Subsumed::CREATE_DB_CONFIG => 262,
            Subsumed::RELATIONAL_EXTENSIONS => 65,
        }
    }

    pub fn reason(self) -> &'static str {
        match self {
            Subsumed::CREATE_DB_CONFIG => concat!(
                "the engine's Pure SQL printer config; its body reads loadDbExtension (the reflective",
                " extension registry) and this platform's compiler is the printer — no code",
                " here consumes a DbConfig value"
            ),
            Subsumed::RELATIONAL_EXTENSIONS => concat!(
                "the engine's extension registry (routing, plan-generation and SQL-printing hooks);",
                " this platform is its own extensions and no arm reads an Extension value"
            ),
        }
    }

    /// `Subsumed.of(calleeFqn)`.
    pub fn of(callee_fqn: Option<&str>) -> Option<Subsumed> {
        let fqn = callee_fqn?;
        Subsumed::VALUES.into_iter().find(|s| s.fqn() == fqn)
    }
}

/// `TdsLegacy`: the legacy TDS functions (and the OLAP ranking ones) the typer recognizes by name.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TdsLegacy {
    AGG,
    COL,
    FUNC,
    WINDOW,
    COLUMN_BY_NAME,
    COLUMN_VALUES,
    OLAP_GROUP_BY,
    PROJECT,
    PROJECT_WITH_COLUMN_SUBSET,
    RENAME_COLUMN,
    RENAME_COLUMNS,
    RESTRICT,
    RESTRICT_DISTINCT,
    TDS_ROWS,
    OLAP_RANK,
    OLAP_DENSE_RANK,
    OLAP_ROW_NUMBER,
    OLAP_AVERAGE_RANK,
}

impl TdsLegacy {
    /// Java's `values()` with each member's name and FQN, in declaration order.
    pub const ROWS: [(TdsLegacy, &'static str, &'static str); 18] = [
        (TdsLegacy::AGG, "AGG", "meta::pure::tds::agg"),
        (TdsLegacy::COL, "COL", "meta::pure::tds::col"),
        (TdsLegacy::FUNC, "FUNC", "meta::pure::tds::func"),
        (TdsLegacy::WINDOW, "WINDOW", "meta::pure::tds::window"),
        (
            TdsLegacy::COLUMN_BY_NAME,
            "COLUMN_BY_NAME",
            "meta::pure::tds::columnByName",
        ),
        (
            TdsLegacy::COLUMN_VALUES,
            "COLUMN_VALUES",
            "meta::pure::tds::columnValues",
        ),
        (
            TdsLegacy::OLAP_GROUP_BY,
            "OLAP_GROUP_BY",
            "meta::pure::tds::olapGroupBy",
        ),
        (TdsLegacy::PROJECT, "PROJECT", "meta::pure::tds::project"),
        (
            TdsLegacy::PROJECT_WITH_COLUMN_SUBSET,
            "PROJECT_WITH_COLUMN_SUBSET",
            "meta::pure::tds::projectWithColumnSubset",
        ),
        (
            TdsLegacy::RENAME_COLUMN,
            "RENAME_COLUMN",
            "meta::pure::tds::renameColumn",
        ),
        (
            TdsLegacy::RENAME_COLUMNS,
            "RENAME_COLUMNS",
            "meta::pure::tds::renameColumns",
        ),
        (TdsLegacy::RESTRICT, "RESTRICT", "meta::pure::tds::restrict"),
        (
            TdsLegacy::RESTRICT_DISTINCT,
            "RESTRICT_DISTINCT",
            "meta::pure::tds::restrictDistinct",
        ),
        (TdsLegacy::TDS_ROWS, "TDS_ROWS", "meta::pure::tds::tdsRows"),
        (
            TdsLegacy::OLAP_RANK,
            "OLAP_RANK",
            "meta::pure::functions::math::olap::rank",
        ),
        (
            TdsLegacy::OLAP_DENSE_RANK,
            "OLAP_DENSE_RANK",
            "meta::pure::functions::math::olap::denseRank",
        ),
        (
            TdsLegacy::OLAP_ROW_NUMBER,
            "OLAP_ROW_NUMBER",
            "meta::pure::functions::math::olap::rowNumber",
        ),
        (
            TdsLegacy::OLAP_AVERAGE_RANK,
            "OLAP_AVERAGE_RANK",
            "meta::pure::functions::math::olap::averageRank",
        ),
    ];

    fn row(self) -> &'static (TdsLegacy, &'static str, &'static str) {
        &TdsLegacy::ROWS[self as usize]
    }

    /// Java's `name()`.
    pub fn name(self) -> &'static str {
        self.row().1
    }

    pub fn fqn(self) -> &'static str {
        self.row().2
    }

    /// The bare name: the FQN's last segment.
    pub fn bare(self) -> &'static str {
        let f = self.fqn();
        &f[f.rfind("::").map_or(0, |i| i + 2)..]
    }

    /// `matches(af)`: the call spells the FQN, or (with no referents) the bare name, or its referents include the FQN.
    pub fn matches(self, af: &p::AppliedFunction) -> bool {
        let name = &af.function;
        if *name == self.fqn() {
            return true;
        }
        if af.referents.is_empty() {
            *name == self.bare()
        } else {
            af.referents.iter().any(|r| *r == self.fqn())
        }
    }
}
