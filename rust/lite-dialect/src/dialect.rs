// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqlDialect` (`core/src/main/java/com/legend/sql/dialect/SqlDialect.java`), the interface the JVM shim keeps:
//! every method, object-safe so the seam (`crate::seam`) holds any dialect. What Java's `AnsiSqlRenderer` implements
//! (the renders, the naming helpers, `raw`) has no default here: a renderer's impl forwards to its
//! [`crate::AnsiSqlRenderer`] methods. Every interface default is a default method with Java's body, so a dialect
//! overrides exactly what its Java class overrides.

use crate::catalog_model::{CatalogColumn, CatalogType, Conversion};
use crate::host_ops::{HostOperations, Refused};
use crate::refusal::Refusal;
use crate::statement::RenderedStatement;
use crate::wire_names;
use lite_sql::{SqlDdl, SqlDml, SqlQuery, SqlType};

/// How a dynamic pivot's probe plan's two statements see one state of the data (Java's `SqlDialect.KeysSnapshot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysSnapshot {
    /// a transaction, the database's own isolation kept
    Transaction,
    /// a transaction at REPEATABLE READ
    RepeatableRead,
    /// a transaction at SERIALIZABLE
    Serializable,
    /// no transaction across statements
    None,
}

impl KeysSnapshot {
    /// The constant's Java `name()`.
    pub fn as_str(self) -> &'static str {
        match self {
            KeysSnapshot::Transaction => "TRANSACTION",
            KeysSnapshot::RepeatableRead => "REPEATABLE_READ",
            KeysSnapshot::Serializable => "SERIALIZABLE",
            KeysSnapshot::None => "NONE",
        }
    }
}

/// A dialect as its callers use it (Java's `SqlDialect`).
pub trait SqlDialect {
    /// Java's `getClass().getSimpleName()`, which the defaults' refusals name.
    fn dialect_name(&self) -> &'static str;

    // ---- the renders (AnsiSqlRenderer's)

    /// Java's `render(SqlQuery)`: the statement as text, refused when it binds a parameter.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal>;
    /// Java's `renderInline(SqlQuery)`: `render` in single-line mode.
    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal>;
    /// Java's `renderStatement(SqlQuery)`.
    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal>;
    /// Java's `render(SqlDdl)`.
    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal>;
    /// Java's `render(SqlDml)`.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal>;
    /// Java's `renderStatement(SqlDml)`.
    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal>;
    /// Java's `raw(statement, writtenFor)`: authored raw SQL in this session's language.
    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal>;
    /// Java's `fromEngineText(statement)`: one statement of the engine's H2 text as this database runs it.
    #[allow(clippy::wrong_self_convention)]
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal>;
    /// Java's `storedName(name, declaredQuoted)`.
    fn stored_name(&self, name: &str, declared_quoted: bool) -> String;
    /// Java's `physicalName(name)`.
    fn physical_name(&self, name: &str) -> String;
    /// Java's `insertColumnName(name, declaredQuoted)`.
    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String;

    // ---- the interface's defaults

    /// The statements a session runs before its first (Java's `sessionSetup`). Default: none.
    fn session_setup(&self) -> Vec<String> {
        Vec::new()
    }

    /// Whether this database has a table only one session sees. Default: it has.
    fn temporary_tables(&self) -> bool {
        true
    }

    /// The statements that make `table` (a `CreateTable`) a table only this session sees, holding `rows` (an
    /// `InsertValues`; none: no rows). Default: the table, then one INSERT of its rows.
    fn session_table(&self, table: &SqlDdl, rows: Option<&SqlDml>) -> Result<Vec<String>, Refusal> {
        let mut out = vec![self.render_ddl(table)?];
        if let Some(rows) = rows {
            out.push(self.render_dml(rows)?);
        }
        Ok(out)
    }

    /// This dialect for the server version a session reports: `None` keeps this one (Java's `return this`).
    /// Default: every version is spelled alike.
    fn for_server(&self, version: &str) -> Option<Box<dyn SqlDialect>> {
        let _ = version;
        None
    }

    /// The server versions this dialect's text is written for; none when it depends on no server's version.
    fn server_versions(&self) -> Option<Vec<String>> {
        None
    }

    /// Whether a target loads its declared rows through the database's own bulk loader. Default: no.
    fn loads_rows_in_bulk(&self) -> bool {
        false
    }

    /// Whether a `RelationalDatabaseConnection` declares this database. Default: relational.
    fn relational(&self) -> bool {
        true
    }

    /// Whether this database runs in this process. Default: a server.
    fn in_process(&self) -> bool {
        false
    }

    /// Whether a program's staged table is dropped with its transaction. Default: it is not.
    fn drops_staged_tables_with_their_transaction(&self) -> bool {
        false
    }

    /// A result column's type as the driver names it, in the wire vocabulary. Default: `WireNames.fold`.
    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        wire_names::fold(driver_type_name)
    }

    /// The wire name a late-bound or pivot-made column is typed by; none where the name alone does not say the type.
    /// Default: `wireTypeName`'s.
    fn typing_wire_name(&self, driver_type_name: Option<&str>) -> Option<String> {
        Some(self.wire_type_name(driver_type_name))
    }

    /// Whether this database delivers a column labelled `label` as the wire type `wire` by a carrier convention of its
    /// own. Default: it has none.
    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        let _ = (label, wire);
        false
    }

    /// This database's keys snapshot. Default: refused, naming the dialect.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Err(Refusal::capability(format!(
            "a dynamic pivot's probe plan reached {}, which says no snapshot for it",
            self.dialect_name()
        )))
    }

    /// The most columns one result answers (`Ok(None)`: no limit, or none known). Default: refused, naming the
    /// dialect.
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Err(Refusal::capability(format!(
            "a dynamic pivot's probe plan reached {}, which says no column limit for it",
            self.dialect_name()
        )))
    }

    /// Whether dynamic PIVOT needs the two-phase staticization pre-pass. Default: no.
    fn needs_static_pivot(&self) -> bool {
        false
    }

    /// The path a written model's specification names for an in-memory session. Default: none.
    fn in_memory_path(&self) -> Option<String> {
        None
    }

    /// This database's host operations. Default: each refused, naming the dialect.
    fn host_operations(&self) -> Box<dyn HostOperations> {
        Box::new(Refused { database: self.dialect_name().to_owned() })
    }

    /// An effect segment's statements as one script. Default: joined by `;\n`, a `;` last.
    fn script(&self, statements: &[String]) -> String {
        format!("{};", statements.join(";\n"))
    }

    /// The statement that aborts a failed script's bracket. Default: none (nothing bracketed).
    fn script_abort(&self) -> Option<String> {
        None
    }

    /// Which statement of a failed script the engine's message names (0-based). Default: the engine does not say.
    fn failing_statement(&self, message: &str, statements: &[String]) -> Option<usize> {
        let _ = (message, statements);
        None
    }

    /// A column as this database's catalog describes it. Default: refused, naming the type and the dialect.
    fn catalog_type(&self, column: &CatalogColumn) -> Result<CatalogType, Refusal> {
        Err(Refusal::capability(format!(
            "reading this database's catalog type '{}' is not implemented for {}",
            column.data_type,
            self.dialect_name()
        )))
    }

    /// The statement whose rows are a table's columns as the catalog reads them. Default: refused
    /// (`UnsupportedOperationException`), naming the dialect.
    fn catalog_columns_query(&self, schema: &str, table: &str) -> Result<String, Refusal> {
        let _ = (schema, table);
        Err(Refusal {
            class: Refusal::UNSUPPORTED_OPERATION,
            message: format!("the platform asks no catalog question of {}", self.dialect_name()),
        })
    }

    /// Whether a store declaring the catalog's name bare reaches it. Default: refused, naming the dialect.
    fn bare_name_reaches(&self, catalog_name: &str) -> Result<bool, Refusal> {
        let _ = catalog_name;
        Err(Refusal::capability(format!(
            "reading this database's catalog names is not implemented for {}",
            self.dialect_name()
        )))
    }

    /// The select list a copy made in this database applies. Default: `*` with no conversion, else refused.
    fn copy_select_list(&self, conversions: &[Conversion]) -> Result<String, Refusal> {
        if conversions.is_empty() {
            return Ok("*".to_owned());
        }
        Err(Refusal::capability(format!(
            "a copy that converts columns in its star select is not implemented for {}",
            self.dialect_name()
        )))
    }
}
