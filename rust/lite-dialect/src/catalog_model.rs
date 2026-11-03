// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a dialect reads its database's own CATALOG into (`SqlDialect.catalogType`): Java's `CatalogModel.Column` and
//! `CatalogModel.Conversion` records with `CatalogModel.sqlIdent`, `CatalogType` with its `Read`, and `CatalogRules`,
//! the one algorithm over a database's catalog decisions. `CatalogModel`'s Database writer (`database`, which calls a
//! dialect's `catalogType` and `bareNameReaches`) stays on the JVM, over any dialect.

use crate::java;
use crate::refusal::Refusal;

/// One catalog column, as the database's catalog describes it (Java's `CatalogModel.Column`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogColumn {
    pub name: String,
    /// the column's own type name, as the catalog writes it: for an alias and for messages, never parsed
    pub data_type: String,
    /// its canonical type, when the catalog names one
    pub logical_type: Option<String>,
    /// a DECIMAL's precision
    pub precision: Option<i32>,
    /// a DECIMAL's scale
    pub scale: Option<i32>,
    /// the catalog says the column holds no NULL
    pub not_null: bool,
}

/// A column's conversion at the source: SQL over the column (Java's `CatalogModel.Conversion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub column: String,
    pub sql: String,
}

/// A name as a SQL identifier, always quoted, its quotes doubled (Java's `CatalogModel.sqlIdent`).
pub fn sql_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// How a source reads a column of a catalog type (Java's `CatalogType.Read`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// the stored value already is the declared type
    AsStored,
    /// converted to be read at all: a source that cannot convert leaves the column out
    Converted,
    /// read in place as stored (under the UTC session), converted in a copy
    CopyConverted,
    /// no Pure Database type holds the value: left out on every source
    LeftOut,
}

impl Read {
    /// The constant's Java `name()`.
    pub fn as_str(self) -> &'static str {
        match self {
            Read::AsStored => "AS_STORED",
            Read::Converted => "CONVERTED",
            Read::CopyConverted => "COPY_CONVERTED",
            Read::LeftOut => "LEFT_OUT",
        }
    }
}

/// A database column type read into what a Pure Database declares, and how a source treats it (Java's `CatalogType`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogType {
    pub read: Read,
    /// the store type declared; absent only for `LEFT_OUT`
    pub declared: Option<String>,
    /// the SQL over `%s` (the column) that converts at the source; exactly for `CONVERTED` and `COPY_CONVERTED`
    pub conversion: Option<String>,
    /// why no Database type holds the column; exactly for `LEFT_OUT`
    pub reason: Option<String>,
}

impl CatalogType {
    /// Java's canonical constructor, with its check that each `read` has exactly its parts.
    pub fn new(
        read: Read,
        declared: Option<String>,
        conversion: Option<String>,
        reason: Option<String>,
    ) -> Result<CatalogType, Refusal> {
        let (declares, converts, says) = (declared.is_some(), conversion.is_some(), reason.is_some());
        let ok = match read {
            Read::AsStored => declares && !converts && !says,
            Read::Converted | Read::CopyConverted => declares && converts && !says,
            Read::LeftOut => !declares && !converts && says,
        };
        if !ok {
            let text = |s: &Option<String>| s.clone().unwrap_or_else(|| "null".to_owned());
            return Err(Refusal::illegal_argument(format!(
                "a {} catalog type with declared={}, conversion={}, reason={}",
                read.as_str(),
                text(&declared),
                text(&conversion),
                text(&reason)
            )));
        }
        Ok(CatalogType { read, declared, conversion, reason })
    }

    pub fn as_stored(declared: &str) -> CatalogType {
        CatalogType { read: Read::AsStored, declared: Some(declared.to_owned()), conversion: None, reason: None }
    }

    pub fn converted(declared: &str, conversion: &str) -> CatalogType {
        CatalogType {
            read: Read::Converted,
            declared: Some(declared.to_owned()),
            conversion: Some(conversion.to_owned()),
            reason: None,
        }
    }

    pub fn copy_converted(declared: &str, conversion: &str) -> CatalogType {
        CatalogType {
            read: Read::CopyConverted,
            declared: Some(declared.to_owned()),
            conversion: Some(conversion.to_owned()),
            reason: None,
        }
    }

    pub fn left_out(reason: &str) -> CatalogType {
        CatalogType { read: Read::LeftOut, declared: None, conversion: None, reason: Some(reason.to_owned()) }
    }
}

/// How a database's dialect reads its own catalog, as data (Java's `CatalogRules`): each map a `LinkedHashMap` in
/// Java, here its entries in order (only looked up).
#[derive(Debug, Clone)]
pub struct CatalogRules {
    /// the database's name, for messages
    pub database: &'static str,
    /// every canonical type the dialect declares, and how
    pub types: Vec<(&'static str, CatalogType)>,
    /// type aliases that are not their canonical type's, by the column's own type name (upper-cased)
    pub aliases: Vec<(&'static str, CatalogType)>,
    /// canonical types refused, each with its reason
    pub refused: Vec<(&'static str, &'static str)>,
    /// the canonical name of the exact decimal type
    pub decimal: &'static str,
    /// how a decimal with no precision and scale is declared; none: refused
    pub unsized_decimal: Option<CatalogType>,
}

fn lookup<'a, V>(entries: &'a [(&'static str, V)], key: &str) -> Option<&'a V> {
    entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

impl CatalogRules {
    /// The column's declaration, or a `DialectCapability` naming why there is none (Java's `typeOf`): its own type
    /// name if an alias (trimmed, then upper-cased), its canonical type if the exact decimal, the canonical type's
    /// decision, else refused with the recorded reason.
    pub fn type_of(&self, column: &CatalogColumn) -> Result<CatalogType, Refusal> {
        if let Some(alias) = lookup(&self.aliases, &java::trim(&column.data_type).to_uppercase()) {
            return Ok(alias.clone());
        }
        let logical = column.logical_type.as_ref().map(|l| l.to_uppercase());
        if logical.as_deref() == Some(self.decimal) {
            if let (Some(precision), Some(scale)) = (column.precision, column.scale) {
                return Ok(CatalogType::as_stored(&format!("DECIMAL({precision},{scale})")));
            }
            if let Some(unsized_decimal) = &self.unsized_decimal {
                return Ok(unsized_decimal.clone());
            }
            return Err(Refusal::capability(format!(
                "a {} column whose catalog gives no precision and scale ('{}') cannot be declared",
                self.decimal, column.data_type
            )));
        }
        if let Some(known) = logical.as_deref().and_then(|l| lookup(&self.types, l)) {
            return Ok(known.clone());
        }
        let why = match logical.as_deref() {
            None => "its catalog names no canonical type",
            Some(l) => lookup(&self.refused, l).copied().unwrap_or("a type this dialect does not read"),
        };
        Err(Refusal::capability(format!(
            "a column of {} type '{}' cannot be declared in a Pure Database ({why})",
            self.database, column.data_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_type_has_exactly_its_reads_parts() {
        let s = |v: &str| Some(v.to_owned());
        assert!(CatalogType::new(Read::AsStored, s("INTEGER"), None, None).is_ok());
        assert!(CatalogType::new(Read::Converted, s("DECIMAL(20,0)"), s("CAST(%s AS DECIMAL(20,0))"), None).is_ok());
        assert!(CatalogType::new(Read::LeftOut, None, None, s("bytes")).is_ok());
        let r = CatalogType::new(Read::AsStored, s("INTEGER"), s("x"), None).unwrap_err();
        assert_eq!(r.class, Refusal::ILLEGAL_ARGUMENT);
        assert_eq!(r.message, "a AS_STORED catalog type with declared=INTEGER, conversion=x, reason=null");
        let r = CatalogType::new(Read::LeftOut, None, None, None).unwrap_err();
        assert_eq!(r.message, "a LEFT_OUT catalog type with declared=null, conversion=null, reason=null");
    }

    #[test]
    fn sql_ident_always_quotes() {
        assert_eq!(sql_ident("items"), "\"items\"");
        assert_eq!(sql_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn rules_read_aliases_decimals_types_and_refusals_in_order() {
        let rules = CatalogRules {
            database: "Db",
            types: vec![("INTEGER", CatalogType::as_stored("INTEGER")), ("JSON", CatalogType::as_stored("X"))],
            aliases: vec![("JSON", CatalogType::as_stored("SEMISTRUCTURED"))],
            refused: vec![("GEOMETRY", "spatial")],
            decimal: "DECIMAL",
            unsized_decimal: Some(CatalogType::as_stored("DECIMAL(38,10)")),
        };
        let col = |data_type: &str, logical: Option<&str>, precision: Option<i32>, scale: Option<i32>| CatalogColumn {
            name: "c".to_owned(),
            data_type: data_type.to_owned(),
            logical_type: logical.map(str::to_owned),
            precision,
            scale,
            not_null: false,
        };
        // the alias wins over the canonical type, read trimmed and upper-cased
        assert_eq!(
            rules.type_of(&col(" json\n", Some("VARCHAR"), None, None)).unwrap().declared.unwrap(),
            "SEMISTRUCTURED"
        );
        assert_eq!(
            rules.type_of(&col("DECIMAL(9,2)", Some("decimal"), Some(9), Some(2))).unwrap().declared.unwrap(),
            "DECIMAL(9,2)"
        );
        assert_eq!(
            rules.type_of(&col("DECIMAL", Some("DECIMAL"), Some(9), None)).unwrap().declared.unwrap(),
            "DECIMAL(38,10)"
        );
        assert_eq!(rules.type_of(&col("INT", Some("integer"), None, None)).unwrap().declared.unwrap(), "INTEGER");
        let refused = |c: CatalogColumn| rules.type_of(&c).unwrap_err().message;
        assert_eq!(
            refused(col("POINT", Some("geometry"), None, None)),
            "a column of Db type 'POINT' cannot be declared in a Pure Database (spatial)"
        );
        assert_eq!(
            refused(col("FOO", Some("FOO"), None, None)),
            "a column of Db type 'FOO' cannot be declared in a Pure Database (a type this dialect does not read)"
        );
        assert_eq!(
            refused(col("FOO", None, None, None)),
            "a column of Db type 'FOO' cannot be declared in a Pure Database (its catalog names no canonical type)"
        );
        let strict = CatalogRules { unsized_decimal: None, ..rules.clone() };
        let r = strict.type_of(&col("DECIMAL", Some("DECIMAL"), None, None)).unwrap_err();
        assert_eq!(r.class, Refusal::DIALECT_CAPABILITY);
        assert_eq!(
            r.message,
            "a DECIMAL column whose catalog gives no precision and scale ('DECIMAL') cannot be declared"
        );
    }
}
