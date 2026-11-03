// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The operations a host runs on its own database, outside any query (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5 and
//! D5): Java's `HostOperation` (the named operations, data only), `HostOperations` (the interface, with `Refused` for a
//! database whose operations are not written), DuckDB's `DuckDbHostOperations`, and the browser's reader of an
//! operation's JSON (`wasm/src/main/java/planner/HostOperationsJson.java`), so the JVM and the browser hand Rust one
//! encoding, refused in the same words.

use crate::duckdb::{DuckDb, IN_MEMORY};
use crate::refusal::Refusal;
use crate::renderer::AnsiSqlRenderer;
use lite_json::{Node, Obj};

/// A relation's name, with its database and schema when given (Java's `HostOperation.Name`). `declared`: a model's own
/// table, spelled as its database holds it (`physicalName`); otherwise a person's or a host's name, always quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
    pub declared: bool,
}

impl Name {
    /// Java's canonical constructor: a model's default schema is no schema (its tables are made with none).
    pub fn new(database: Option<String>, schema: Option<String>, name: String, declared: bool) -> Name {
        let schema = if declared && schema.as_deref() == Some("default") { None } else { schema };
        Name { database, schema, name, declared }
    }
}

/// Where a relation's rows come from (Java's `HostOperation.Source`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Relation(Name),
    File { at: String, format: FileFormat },
    Remote { url: String, format: RemoteFormat },
    Query { sql: String },
}

/// How a file is read (Java's `HostOperation.FileFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Csv,
    CsvText,
    Parquet,
    Json,
}

/// How a remote file is read (Java's `HostOperation.RemoteFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFormat {
    Csv,
    Parquet,
    Iceberg,
}

/// A relation's kind (Java's `HostOperation.Kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Table,
    View,
}

/// A column replaced as a copy is made (Java's `HostOperation.Conversion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub column: String,
    pub sql: String,
}

/// One column put in: the declared column, and the source's column it is read from (Java's `HostOperation.Column`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub target: String,
    pub from: String,
}

/// A transaction's step (Java's `HostOperation.Transaction.Step`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Begin,
    Commit,
    Rollback,
}

/// A named operation a host runs on its own database (Java's sealed `HostOperation`), each record a variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOperation {
    MakeRelation {
        kind: Kind,
        target: Name,
        replace: bool,
        select_list: Option<String>,
        replacing: Vec<Conversion>,
        from: Source,
    },
    Insert {
        target: Name,
        columns: Vec<Column>,
        from: Source,
    },
    Drop {
        kind: Kind,
        target: Name,
    },
    MakeSchema {
        database: Option<String>,
        name: String,
    },
    DropSchema {
        database: Option<String>,
        name: String,
    },
    Count {
        from: Source,
    },
    /// its credentials in the order given (Java's `LinkedHashMap`)
    Secret {
        name: String,
        scope: String,
        credentials: Vec<(String, String)>,
    },
    Extensions {
        names: Vec<String>,
    },
    ExtensionRepository {
        url: String,
    },
    ExtensionDirectory {
        folder: String,
    },
    FileSearchPath {
        folder: String,
    },
    Transaction {
        step: Step,
    },
    NameTaken {
        name: String,
    },
    RelationExists {
        name: Name,
    },
    TablesIn {
        database: Option<String>,
        schema: String,
    },
    SchemaExists {
        database: Option<String>,
        schema: String,
    },
    CurrentDatabase,
    Platform,
    IcebergManifestLists {
        url: String,
    },
    IcebergManifests {
        lists: Vec<String>,
    },
    IcebergDataFiles {
        url: String,
    },
}

impl HostOperation {
    /// The record's `getClass().getSimpleName()`, which a refusal names.
    pub fn simple_name(&self) -> &'static str {
        match self {
            HostOperation::MakeRelation { .. } => "MakeRelation",
            HostOperation::Insert { .. } => "Insert",
            HostOperation::Drop { .. } => "Drop",
            HostOperation::MakeSchema { .. } => "MakeSchema",
            HostOperation::DropSchema { .. } => "DropSchema",
            HostOperation::Count { .. } => "Count",
            HostOperation::Secret { .. } => "Secret",
            HostOperation::Extensions { .. } => "Extensions",
            HostOperation::ExtensionRepository { .. } => "ExtensionRepository",
            HostOperation::ExtensionDirectory { .. } => "ExtensionDirectory",
            HostOperation::FileSearchPath { .. } => "FileSearchPath",
            HostOperation::Transaction { .. } => "Transaction",
            HostOperation::NameTaken { .. } => "NameTaken",
            HostOperation::RelationExists { .. } => "RelationExists",
            HostOperation::TablesIn { .. } => "TablesIn",
            HostOperation::SchemaExists { .. } => "SchemaExists",
            HostOperation::CurrentDatabase => "CurrentDatabase",
            HostOperation::Platform => "Platform",
            HostOperation::IcebergManifestLists { .. } => "IcebergManifestLists",
            HostOperation::IcebergManifests { .. } => "IcebergManifests",
            HostOperation::IcebergDataFiles { .. } => "IcebergDataFiles",
        }
    }
}

/// A database's host operations (Java's `HostOperations`).
pub trait HostOperations {
    /// The statements that make the database `name`, empty.
    fn open_database(&self, name: &str) -> Result<Vec<String>, Refusal>;
    /// The statements that point a connection of its own at `name`.
    fn use_database(&self, name: &str) -> Result<Vec<String>, Refusal>;
    /// The statements that drop `name` with everything in it.
    fn close_database(&self, name: &str) -> Result<Vec<String>, Refusal>;
    /// A named operation's statements, in order (none when it has nothing to do), or a refusal naming it.
    fn statements(&self, operation: &HostOperation) -> Result<Vec<String>, Refusal>;
}

/// `operation` refused on `database`: not written for it (Java's `HostOperations.refusal`).
pub fn refusal(operation: &HostOperation, database: &str) -> Refusal {
    Refusal::capability(format!(
        "a host's {} on {database} is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)",
        operation.simple_name()
    ))
}

/// A database whose host operations are not written yet: each refused, by name (Java's `HostOperations.Refused`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub database: String,
}

impl Refused {
    fn refused(&self) -> Refusal {
        Refusal::capability(format!(
            "a project's own database on {} is not built: only DuckDB makes one so far \
             (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)",
            self.database
        ))
    }
}

impl HostOperations for Refused {
    fn open_database(&self, _name: &str) -> Result<Vec<String>, Refusal> {
        Err(self.refused())
    }

    fn use_database(&self, _name: &str) -> Result<Vec<String>, Refusal> {
        Err(self.refused())
    }

    fn close_database(&self, _name: &str) -> Result<Vec<String>, Refusal> {
        Err(self.refused())
    }

    fn statements(&self, operation: &HostOperation) -> Result<Vec<String>, Refusal> {
        Err(refusal(operation, &self.database))
    }
}

/// DuckDB's host operations (Java's `DuckDbHostOperations`): a project's own database is an in-memory database
/// ATTACHed to the host's, a connection points at it with USE, DETACH drops it; the named operations are DuckDB's
/// readers, `CREATE ... AS SELECT`, `SELECT * REPLACE`, its secrets, its extensions' settings and its catalog
/// functions. A model's declared name is spelled as its seed made it (DuckDB's `physicalName`); every other name is
/// quoted; every value is a literal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DuckDbHostOperations;

/// S3 credentials' names as a secret takes them, sorted (Java's `new TreeSet<>(CREDENTIALS)`, which its refusal
/// prints); `USE_SSL` a boolean, every other a string.
const CREDENTIALS: [&str; 7] = ["ENDPOINT", "KEY_ID", "REGION", "SECRET", "SESSION_TOKEN", "URL_STYLE", "USE_SSL"];

impl HostOperations for DuckDbHostOperations {
    fn open_database(&self, name: &str) -> Result<Vec<String>, Refusal> {
        Ok(vec![format!("ATTACH '{IN_MEMORY}' AS {}", quoted(name))])
    }

    fn use_database(&self, name: &str) -> Result<Vec<String>, Refusal> {
        Ok(vec![format!("USE {}", quoted(name))])
    }

    fn close_database(&self, name: &str) -> Result<Vec<String>, Refusal> {
        Ok(vec![format!("DETACH DATABASE IF EXISTS {}", quoted(name))])
    }

    fn statements(&self, operation: &HostOperation) -> Result<Vec<String>, Refusal> {
        Ok(match operation {
            HostOperation::MakeRelation { kind, target, replace, select_list, replacing, from } => vec![format!(
                "CREATE {}{} {} AS SELECT {} FROM {}",
                if *replace { "OR REPLACE " } else { "" },
                kind_text(*kind),
                name(target),
                columns(select_list.as_deref(), replacing),
                source(from)
            )],
            HostOperation::Insert { target, columns, from } => vec![format!(
                "INSERT INTO {} ({}) SELECT {} FROM {}",
                name(target),
                columns.iter().map(|c| physical_name(&c.target)).collect::<Vec<_>>().join(", "),
                columns.iter().map(|c| quoted(&c.from)).collect::<Vec<_>>().join(", "),
                source(from)
            )],
            HostOperation::Drop { kind, target } => {
                vec![format!("DROP {} IF EXISTS {}", kind_text(*kind), name(target))]
            }
            HostOperation::MakeSchema { database, name } => {
                vec![format!("CREATE SCHEMA IF NOT EXISTS {}", schema(database.as_deref(), name))]
            }
            HostOperation::DropSchema { database, name } => {
                vec![format!("DROP SCHEMA IF EXISTS {}", schema(database.as_deref(), name))]
            }
            HostOperation::Count { from } => vec![format!("SELECT count(*) AS n FROM {}", source(from))],
            HostOperation::Secret { name, scope, credentials } => secret(name, scope, credentials)?,
            HostOperation::Extensions { names } => {
                let mut out = Vec::new();
                for n in names {
                    let extension = extension_name(n)?;
                    out.push(format!("INSTALL {extension}"));
                    out.push(format!("LOAD {extension}"));
                }
                out
            }
            HostOperation::ExtensionRepository { url } => vec![
                format!("SET custom_extension_repository = {}", literal(url)),
                format!("SET autoinstall_extension_repository = {}", literal(url)),
            ],
            HostOperation::ExtensionDirectory { folder } => vec![
                format!("SET extension_directory = {}", literal(folder)),
                format!("SET custom_extension_repository = {}", literal(folder)),
                "SET autoinstall_known_extensions = false".to_owned(),
            ],
            HostOperation::FileSearchPath { folder } => vec![format!("SET file_search_path = {}", literal(folder))],
            HostOperation::Transaction { step } => vec![
                match step {
                    Step::Begin => "BEGIN TRANSACTION",
                    Step::Commit => "COMMIT",
                    Step::Rollback => "ROLLBACK",
                }
                .to_owned(),
            ],
            HostOperation::NameTaken { name } => vec![format!(
                "SELECT (SELECT count(*) FROM duckdb_tables() WHERE database_name IN (current_database(), 'temp') \
                 AND lower(table_name) = lower({0})) + (SELECT count(*) FROM duckdb_views() WHERE NOT internal \
                 AND database_name IN (current_database(), 'temp') AND lower(view_name) = lower({0})) AS n",
                literal(name)
            )],
            HostOperation::RelationExists { name } => {
                let r#where = format!(
                    "{} AND lower(schema_name) = lower({})",
                    database_is(name.database.as_deref()),
                    literal(name.schema.as_deref().unwrap_or("main"))
                );
                vec![format!(
                    "SELECT (SELECT count(*) FROM duckdb_tables() WHERE {0} AND lower(table_name) = lower({1})) \
                     + (SELECT count(*) FROM duckdb_views() WHERE NOT internal AND {0} \
                     AND lower(view_name) = lower({1})) AS n",
                    r#where,
                    literal(&name.name)
                )]
            }
            HostOperation::TablesIn { database, schema } => vec![format!(
                "SELECT count(*) AS n FROM duckdb_tables() WHERE {} AND lower(schema_name) = lower({})",
                database_is(database.as_deref()),
                literal(schema)
            )],
            HostOperation::SchemaExists { database, schema } => vec![format!(
                "SELECT count(*) AS n FROM duckdb_schemas() WHERE {} AND lower(schema_name) = lower({})",
                database_is(database.as_deref()),
                literal(schema)
            )],
            HostOperation::CurrentDatabase => vec!["SELECT current_database() AS name".to_owned()],
            HostOperation::Platform => vec!["PRAGMA platform".to_owned()],
            HostOperation::IcebergManifestLists { url } => {
                vec![format!("SELECT manifest_list AS path FROM iceberg_snapshots({})", literal(url))]
            }
            HostOperation::IcebergManifests { lists } => vec![format!(
                "SELECT DISTINCT manifest_path AS path FROM read_avro([{}])",
                lists.iter().map(|l| literal(l)).collect::<Vec<_>>().join(", ")
            )],
            HostOperation::IcebergDataFiles { url } => {
                vec![format!("SELECT file_path AS path FROM iceberg_metadata({})", literal(url))]
            }
        })
    }
}

fn kind_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Table => "TABLE",
        Kind::View => "VIEW",
    }
}

/// DuckDB's `physicalName` (Java's `DIALECT.physicalName`, a `DuckDb` of its own).
fn physical_name(name: &str) -> String {
    AnsiSqlRenderer::physical_name(&DuckDb::new(), name)
}

/// A relation made's columns: the planner's copy select list, every column with conversions in place, or all.
fn columns(select_list: Option<&str>, replacing: &[Conversion]) -> String {
    if let Some(list) = select_list {
        return list.to_owned();
    }
    if replacing.is_empty() {
        return "*".to_owned();
    }
    let items: Vec<String> = replacing.iter().map(|c| format!("{} AS {}", c.sql, quoted(&c.column))).collect();
    format!("* REPLACE ({})", items.join(", "))
}

fn source(source: &Source) -> String {
    match source {
        Source::Relation(n) => name(n),
        Source::File { at, format } => match format {
            // the sniffer takes delimiter, quoting and types; upstream's DataCube asks for a header, as this does
            FileFormat::Csv => format!("read_csv({}, AUTO_DETECT=TRUE, HEADER=TRUE)", literal(at)),
            // a declared table's file: every column as text, each converted to its column's type as it goes in
            FileFormat::CsvText => format!("read_csv({}, header = true, all_varchar = true)", literal(at)),
            FileFormat::Parquet => format!("read_parquet({})", literal(at)),
            // each record's top-level keys its columns; what is nested below them a STRUCT or a LIST, kept
            FileFormat::Json => format!("read_json({}, auto_detect=true)", literal(at)),
        },
        Source::Remote { url, format } => match format {
            RemoteFormat::Csv => format!("read_csv_auto({})", literal(url)),
            RemoteFormat::Parquet => format!("read_parquet({})", literal(url)),
            RemoteFormat::Iceberg => format!("iceberg_scan({})", literal(url)),
        },
        Source::Query { sql } => format!("({sql})"),
    }
}

/// A secret for S3, scoped to a source's own path; none when there are no credentials at all.
fn secret(name: &str, scope: &str, credentials: &[(String, String)]) -> Result<Vec<String>, Refusal> {
    let mut parts = vec!["TYPE S3".to_owned()];
    for (key, value) in credentials {
        if !CREDENTIALS.contains(&key.as_str()) {
            return Err(Refusal::illegal_argument(format!(
                "a secret's credential is one of [{}], not {key}",
                CREDENTIALS.join(", ")
            )));
        }
        // Boolean.parseBoolean: "true" in any case, anything else false
        let spelled = if key == "USE_SSL" { value.eq_ignore_ascii_case("true").to_string() } else { literal(value) };
        parts.push(format!("{key} {spelled}"));
    }
    if parts.len() == 1 {
        return Ok(Vec::new());
    }
    parts.push(format!("SCOPE {}", literal(scope)));
    Ok(vec![format!("CREATE OR REPLACE SECRET {} ({})", quoted(name), parts.join(", "))])
}

/// An extension's name, as INSTALL takes it: `[a-z][a-z0-9_]*`, never text a host spliced in.
fn extension_name(name: &str) -> Result<&str, Refusal> {
    let mut bytes = name.bytes();
    let word = matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !word {
        return Err(Refusal::illegal_argument(format!(
            "an extension's name is a word of lower-case letters, digits and '_', not {name}"
        )));
    }
    Ok(name)
}

/// A catalog question's database: the one named, or the current one.
fn database_is(database: Option<&str>) -> String {
    match database {
        None => "database_name = current_database()".to_owned(),
        Some(d) => format!("database_name = {}", literal(d)),
    }
}

/// A relation's name: a model's declared one as its seed spelled it, any other quoted; its database, schema too.
fn name(n: &Name) -> String {
    let spell = |s: &str| if n.declared { physical_name(s) } else { quoted(s) };
    let mut out = String::new();
    if let Some(database) = &n.database {
        out.push_str(&quoted(database));
        out.push('.');
    }
    if let Some(schema) = &n.schema {
        out.push_str(&spell(schema));
        out.push('.');
    }
    out.push_str(&spell(&n.name));
    out
}

fn schema(database: Option<&str>, name: &str) -> String {
    match database {
        None => quoted(name),
        Some(d) => format!("{}.{}", quoted(d), quoted(name)),
    }
}

/// A name as DuckDB reads it, quoted: a host's or a person's name, never trusted to need none.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A value as DuckDB reads it: a string literal (no NUL splicing: Java's `literal`, not the renderer's `stringLit`).
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

// ==================================================================
// The browser's JSON (Java's planner.HostOperationsJson): each field by its kind, anything else refused by name
// ==================================================================

/// One operation's JSON, read as `HostOperationsJson.operation` reads it (`node` refused when it is no object).
pub fn read_operation(node: &Node) -> Result<HostOperation, Refusal> {
    let o = object(node, "an operation")?;
    let op = string(o, "op")?;
    Ok(match op.as_str() {
        "relation" => HostOperation::MakeRelation {
            kind: kind(&string(o, "kind")?)?,
            target: read_name(object_field(o, "target")?)?,
            replace: bool_field(o, "replace")?,
            select_list: string_or_null(o, "selectList")?,
            replacing: conversions(o)?,
            from: read_source(object_field(o, "from")?)?,
        },
        "insert" => HostOperation::Insert {
            target: read_name(object_field(o, "target")?)?,
            columns: read_columns(o)?,
            from: read_source(object_field(o, "from")?)?,
        },
        "drop" => {
            HostOperation::Drop { kind: kind(&string(o, "kind")?)?, target: read_name(object_field(o, "target")?)? }
        }
        "schema" => HostOperation::MakeSchema { database: string_or_null(o, "database")?, name: string(o, "name")? },
        "dropSchema" => {
            HostOperation::DropSchema { database: string_or_null(o, "database")?, name: string(o, "name")? }
        }
        "count" => HostOperation::Count { from: read_source(object_field(o, "from")?)? },
        "secret" => HostOperation::Secret {
            name: string(o, "name")?,
            scope: string(o, "scope")?,
            credentials: credentials(object_field(o, "credentials")?)?,
        },
        "extensions" => HostOperation::Extensions { names: string_list(o, "names")? },
        "extensionRepository" => HostOperation::ExtensionRepository { url: string(o, "url")? },
        "extensionDirectory" => HostOperation::ExtensionDirectory { folder: string(o, "folder")? },
        "fileSearchPath" => HostOperation::FileSearchPath { folder: string(o, "folder")? },
        "transaction" => HostOperation::Transaction {
            step: match string(o, "step")?.as_str() {
                "begin" => Step::Begin,
                "commit" => Step::Commit,
                "rollback" => Step::Rollback,
                other => {
                    return Err(Refusal::illegal_argument(format!(
                        "a transaction's step is begin, commit or rollback, not {other}"
                    )));
                }
            },
        },
        "nameTaken" => HostOperation::NameTaken { name: string(o, "name")? },
        "relationExists" => HostOperation::RelationExists { name: read_name(object_field(o, "name")?)? },
        "tablesIn" => {
            HostOperation::TablesIn { database: string_or_null(o, "database")?, schema: string(o, "schema")? }
        }
        "schemaExists" => {
            HostOperation::SchemaExists { database: string_or_null(o, "database")?, schema: string(o, "schema")? }
        }
        "currentDatabase" => HostOperation::CurrentDatabase,
        "platform" => HostOperation::Platform,
        "icebergManifestLists" => HostOperation::IcebergManifestLists { url: string(o, "url")? },
        "icebergManifests" => HostOperation::IcebergManifests { lists: string_list(o, "lists")? },
        "icebergDataFiles" => HostOperation::IcebergDataFiles { url: string(o, "url")? },
        other => {
            return Err(Refusal::illegal_argument(format!(
                "a host's operation is not one this platform names: {other}"
            )));
        }
    })
}

fn kind(kind: &str) -> Result<Kind, Refusal> {
    match kind {
        "table" => Ok(Kind::Table),
        "view" => Ok(Kind::View),
        other => Err(Refusal::illegal_argument(format!("a relation's kind is table or view, not {other}"))),
    }
}

fn read_name(o: &Obj) -> Result<Name, Refusal> {
    Ok(Name::new(
        string_or_null(o, "database")?,
        string_or_null(o, "schema")?,
        string(o, "name")?,
        bool_field(o, "declared")?,
    ))
}

fn read_source(o: &Obj) -> Result<Source, Refusal> {
    if o.get("relation").is_some() {
        return Ok(Source::Relation(read_name(object_field(o, "relation")?)?));
    }
    if o.get("file").is_some() {
        let f = object_field(o, "file")?;
        let format = string(f, "format")?;
        let at = string(f, "at")?;
        let format = match format.as_str() {
            "csv" => FileFormat::Csv,
            "csvText" => FileFormat::CsvText,
            "parquet" => FileFormat::Parquet,
            "json" => FileFormat::Json,
            other => {
                return Err(Refusal::illegal_argument(format!(
                    "a file is read as csv, csvText, parquet or json, not {other}"
                )));
            }
        };
        return Ok(Source::File { at, format });
    }
    if o.get("remote").is_some() {
        let r = object_field(o, "remote")?;
        let format = string(r, "format")?;
        let url = string(r, "url")?;
        let format = match format.as_str() {
            "csv" => RemoteFormat::Csv,
            "parquet" => RemoteFormat::Parquet,
            "iceberg" => RemoteFormat::Iceberg,
            other => {
                return Err(Refusal::illegal_argument(format!(
                    "a remote file is csv, parquet or iceberg, not {other}"
                )));
            }
        };
        return Ok(Source::Remote { url, format });
    }
    if o.get("query").is_some() {
        return Ok(Source::Query { sql: string(o, "query")? });
    }
    Err(Refusal::illegal_argument("a source is a relation, a file, a remote file or a query"))
}

fn conversions(o: &Obj) -> Result<Vec<Conversion>, Refusal> {
    let Some(replacing) = o.get("replacing") else {
        return Ok(Vec::new());
    };
    let Node::Arr(items) = replacing else {
        return Err(Refusal::illegal_argument("a relation's replacing is an array"));
    };
    items
        .iter()
        .map(|n| {
            let c = object(n, "a conversion")?;
            Ok(Conversion { column: string(c, "column")?, sql: string(c, "sql")? })
        })
        .collect()
}

fn read_columns(o: &Obj) -> Result<Vec<Column>, Refusal> {
    let Some(Node::Arr(items)) = o.get("columns") else {
        return Err(Refusal::illegal_argument("rows put in name their columns, an array"));
    };
    items
        .iter()
        .map(|n| {
            let c = object(n, "a column")?;
            Ok(Column { target: string(c, "target")?, from: string(c, "from")? })
        })
        .collect()
}

fn credentials(o: &Obj) -> Result<Vec<(String, String)>, Refusal> {
    o.iter()
        .map(|(key, value)| match value {
            Node::Str(s) => Ok((key.to_owned(), s.clone())),
            _ => Err(Refusal::illegal_argument(format!("a credential is a string: {key}"))),
        })
        .collect()
}

fn string_list(o: &Obj, field: &str) -> Result<Vec<String>, Refusal> {
    let refused = || Refusal::illegal_argument(format!("an operation's {field} is an array of strings"));
    let Some(Node::Arr(items)) = o.get(field) else {
        return Err(refused());
    };
    items
        .iter()
        .map(|n| match n {
            Node::Str(s) => Ok(s.clone()),
            _ => Err(refused()),
        })
        .collect()
}

fn object<'a>(n: &'a Node, what: &str) -> Result<&'a Obj, Refusal> {
    match n {
        Node::Obj(o) => Ok(o),
        _ => Err(Refusal::illegal_argument(format!("{what} is a JSON object"))),
    }
}

fn object_field<'a>(o: &'a Obj, field: &str) -> Result<&'a Obj, Refusal> {
    match o.get(field) {
        None => Err(Refusal::illegal_argument(format!("an operation names no {field}"))),
        Some(n) => object(n, &format!("an operation's {field}")),
    }
}

fn string(o: &Obj, field: &str) -> Result<String, Refusal> {
    match o.get(field) {
        Some(Node::Str(s)) => Ok(s.clone()),
        _ => Err(Refusal::illegal_argument(format!("an operation's {field} is a string"))),
    }
}

fn string_or_null(o: &Obj, field: &str) -> Result<Option<String>, Refusal> {
    match o.get(field) {
        None | Some(Node::Null) => Ok(None),
        Some(_) => string(o, field).map(Some),
    }
}

fn bool_field(o: &Obj, field: &str) -> Result<bool, Refusal> {
    match o.get(field) {
        None => Ok(false),
        Some(Node::Bool(b)) => Ok(*b),
        Some(_) => Err(Refusal::illegal_argument(format!("an operation's {field} is true or false"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(json: &str) -> Result<HostOperation, Refusal> {
        read_operation(&lite_json::parse(json).expect("JSON"))
    }

    fn statements(json: &str) -> Vec<String> {
        DuckDbHostOperations.statements(&op(json).expect("an operation")).expect("statements")
    }

    fn refused(json: &str) -> String {
        match op(json).and_then(|o| DuckDbHostOperations.statements(&o)) {
            Ok(s) => panic!("{json} answered {s:?}"),
            Err(r) => {
                assert_eq!(r.class, Refusal::ILLEGAL_ARGUMENT, "{r}");
                r.message
            }
        }
    }

    #[test]
    fn a_project_database_is_attached_used_and_detached() {
        let ops = DuckDbHostOperations;
        assert_eq!(ops.open_database("p\"1").unwrap(), ["ATTACH ':memory:' AS \"p\"\"1\""]);
        assert_eq!(ops.use_database("p1").unwrap(), ["USE \"p1\""]);
        assert_eq!(ops.close_database("p1").unwrap(), ["DETACH DATABASE IF EXISTS \"p1\""]);
    }

    #[test]
    fn relations_are_made_inserted_dropped_and_counted() {
        assert_eq!(
            statements(
                r#"{"op":"relation","kind":"table","target":{"name":"orders","schema":"default","declared":true},
                "from":{"file":{"at":"o'1.csv","format":"csv"}}}"#
            ),
            ["CREATE TABLE orders AS SELECT * FROM read_csv('o''1.csv', AUTO_DETECT=TRUE, HEADER=TRUE)"]
        );
        assert_eq!(
            statements(
                r#"{"op":"relation","kind":"view","replace":true,"target":{"database":"db","schema":"s","name":"v"},
                "replacing":[{"column":"a\"b","sql":"to_json(\"a\")"}],"from":{"remote":{"url":"s3://x","format":"iceberg"}}}"#
            ),
            [
                "CREATE OR REPLACE VIEW \"db\".\"s\".\"v\" AS SELECT * REPLACE (to_json(\"a\") AS \"a\"\"b\") FROM iceberg_scan('s3://x')"
            ]
        );
        assert_eq!(
            statements(
                r#"{"op":"relation","kind":"table","target":{"name":"t"},"selectList":"a, b","replacing":[{"column":"x","sql":"y"}],
                "from":{"query":"SELECT 1"}}"#
            ),
            ["CREATE TABLE \"t\" AS SELECT a, b FROM (SELECT 1)"]
        );
        assert_eq!(
            statements(
                r#"{"op":"insert","target":{"schema":"Sales","name":"order","declared":true},
                "columns":[{"target":"id","from":"Id"},{"target":"select","from":"s\"x"}],"from":{"file":{"at":"f","format":"csvText"}}}"#
            ),
            [
                "INSERT INTO Sales.\"order\" (id, \"select\") SELECT \"Id\", \"s\"\"x\" FROM read_csv('f', header = true, all_varchar = true)"
            ]
        );
        assert_eq!(statements(r#"{"op":"drop","kind":"view","target":{"name":"v"}}"#), ["DROP VIEW IF EXISTS \"v\""]);
        assert_eq!(statements(r#"{"op":"schema","name":"s"}"#), ["CREATE SCHEMA IF NOT EXISTS \"s\""]);
        assert_eq!(
            statements(r#"{"op":"dropSchema","database":"d","name":"s"}"#),
            ["DROP SCHEMA IF EXISTS \"d\".\"s\""]
        );
        assert_eq!(
            statements(r#"{"op":"count","from":{"relation":{"name":"t"}}}"#),
            ["SELECT count(*) AS n FROM \"t\""]
        );
        assert_eq!(
            statements(r#"{"op":"count","from":{"file":{"at":"x.parquet","format":"parquet"}}}"#),
            ["SELECT count(*) AS n FROM read_parquet('x.parquet')"]
        );
        assert_eq!(
            statements(r#"{"op":"count","from":{"file":{"at":"x.json","format":"json"}}}"#),
            ["SELECT count(*) AS n FROM read_json('x.json', auto_detect=true)"]
        );
        assert_eq!(
            statements(r#"{"op":"count","from":{"remote":{"url":"u","format":"csv"}}}"#),
            ["SELECT count(*) AS n FROM read_csv_auto('u')"]
        );
    }

    #[test]
    fn secrets_extensions_settings_and_transactions() {
        assert_eq!(
            statements(
                r#"{"op":"secret","name":"s","scope":"s3://b/","credentials":{"KEY_ID":"k'","USE_SSL":"TRUE","REGION":"r"}}"#
            ),
            ["CREATE OR REPLACE SECRET \"s\" (TYPE S3, KEY_ID 'k''', USE_SSL true, REGION 'r', SCOPE 's3://b/')"]
        );
        assert_eq!(
            statements(r#"{"op":"secret","name":"s","scope":"x","credentials":{"USE_SSL":"yes"}}"#),
            ["CREATE OR REPLACE SECRET \"s\" (TYPE S3, USE_SSL false, SCOPE 'x')"]
        );
        assert!(statements(r#"{"op":"secret","name":"s","scope":"x","credentials":{}}"#).is_empty());
        assert_eq!(
            refused(r#"{"op":"secret","name":"s","scope":"x","credentials":{"PASSWORD":"p"}}"#),
            "a secret's credential is one of [ENDPOINT, KEY_ID, REGION, SECRET, SESSION_TOKEN, URL_STYLE, USE_SSL], not \
             PASSWORD"
        );
        assert_eq!(
            statements(r#"{"op":"extensions","names":["httpfs","iceberg_2"]}"#),
            ["INSTALL httpfs", "LOAD httpfs", "INSTALL iceberg_2", "LOAD iceberg_2"]
        );
        assert!(statements(r#"{"op":"extensions","names":[]}"#).is_empty());
        assert_eq!(
            refused(r#"{"op":"extensions","names":["ok","Bad"]}"#),
            "an extension's name is a word of lower-case letters, digits and '_', not Bad"
        );
        assert_eq!(
            statements(r#"{"op":"extensionRepository","url":"https://x/'"}"#),
            [
                "SET custom_extension_repository = 'https://x/'''",
                "SET autoinstall_extension_repository = 'https://x/'''"
            ]
        );
        assert_eq!(
            statements(r#"{"op":"extensionDirectory","folder":"/e"}"#),
            [
                "SET extension_directory = '/e'",
                "SET custom_extension_repository = '/e'",
                "SET autoinstall_known_extensions = false"
            ]
        );
        assert_eq!(statements(r#"{"op":"fileSearchPath","folder":"/d"}"#), ["SET file_search_path = '/d'"]);
        assert_eq!(statements(r#"{"op":"transaction","step":"begin"}"#), ["BEGIN TRANSACTION"]);
        assert_eq!(statements(r#"{"op":"transaction","step":"commit"}"#), ["COMMIT"]);
        assert_eq!(statements(r#"{"op":"transaction","step":"rollback"}"#), ["ROLLBACK"]);
        assert_eq!(
            refused(r#"{"op":"transaction","step":"abort"}"#),
            "a transaction's step is begin, commit or rollback, not abort"
        );
    }

    #[test]
    fn catalog_questions() {
        assert_eq!(
            statements(r#"{"op":"nameTaken","name":"T'"}"#),
            ["SELECT (SELECT count(*) FROM duckdb_tables() WHERE database_name IN (current_database(), 'temp') AND \
                 lower(table_name) = lower('T''')) + (SELECT count(*) FROM duckdb_views() WHERE NOT internal AND \
                 database_name IN (current_database(), 'temp') AND lower(view_name) = lower('T''')) AS n"]
        );
        assert_eq!(
            statements(r#"{"op":"relationExists","name":{"name":"t","schema":"default","declared":true}}"#),
            ["SELECT (SELECT count(*) FROM duckdb_tables() WHERE database_name = current_database() AND \
                 lower(schema_name) = lower('main') AND lower(table_name) = lower('t')) + (SELECT count(*) FROM \
                 duckdb_views() WHERE NOT internal AND database_name = current_database() AND lower(schema_name) = \
                 lower('main') AND lower(view_name) = lower('t')) AS n"]
        );
        assert_eq!(
            statements(r#"{"op":"relationExists","name":{"database":"d","schema":"default","name":"t"}}"#),
            ["SELECT (SELECT count(*) FROM duckdb_tables() WHERE database_name = 'd' AND lower(schema_name) = \
                 lower('default') AND lower(table_name) = lower('t')) + (SELECT count(*) FROM duckdb_views() WHERE \
                 NOT internal AND database_name = 'd' AND lower(schema_name) = lower('default') AND \
                 lower(view_name) = lower('t')) AS n"]
        );
        assert_eq!(
            statements(r#"{"op":"tablesIn","schema":"s"}"#),
            [
                "SELECT count(*) AS n FROM duckdb_tables() WHERE database_name = current_database() AND lower(schema_name) = lower('s')"
            ]
        );
        assert_eq!(
            statements(r#"{"op":"schemaExists","database":"d","schema":"s"}"#),
            [
                "SELECT count(*) AS n FROM duckdb_schemas() WHERE database_name = 'd' AND lower(schema_name) = lower('s')"
            ]
        );
        assert_eq!(statements(r#"{"op":"currentDatabase"}"#), ["SELECT current_database() AS name"]);
        assert_eq!(statements(r#"{"op":"platform"}"#), ["PRAGMA platform"]);
        assert_eq!(
            statements(r#"{"op":"icebergManifestLists","url":"u"}"#),
            ["SELECT manifest_list AS path FROM iceberg_snapshots('u')"]
        );
        assert_eq!(
            statements(r#"{"op":"icebergManifests","lists":["a","b'"]}"#),
            ["SELECT DISTINCT manifest_path AS path FROM read_avro(['a', 'b'''])"]
        );
        assert_eq!(
            statements(r#"{"op":"icebergDataFiles","url":"u"}"#),
            ["SELECT file_path AS path FROM iceberg_metadata('u')"]
        );
    }

    #[test]
    fn the_reader_refuses_as_the_browsers_does() {
        let cases = [
            (r#"[]"#, "an operation is a JSON object"),
            (r#"{}"#, "an operation's op is a string"),
            (r#"{"op":"nope"}"#, "a host's operation is not one this platform names: nope"),
            (r#"{"op":"drop","kind":"index","target":{"name":"t"}}"#, "a relation's kind is table or view, not index"),
            (r#"{"op":"drop","kind":"table"}"#, "an operation names no target"),
            (r#"{"op":"drop","kind":"table","target":[]}"#, "an operation's target is a JSON object"),
            (r#"{"op":"drop","kind":"table","target":{"name":1}}"#, "an operation's name is a string"),
            (
                r#"{"op":"drop","kind":"table","target":{"name":"t","declared":"yes"}}"#,
                "an operation's declared is true or false",
            ),
            (r#"{"op":"count","from":{}}"#, "a source is a relation, a file, a remote file or a query"),
            (r#"{"op":"count","from":{"relation":null}}"#, "an operation's relation is a JSON object"),
            (r#"{"op":"count","from":{"file":{"at":"a"}}}"#, "an operation's format is a string"),
            (r#"{"op":"count","from":{"file":{"format":"xml"}}}"#, "an operation's at is a string"),
            (
                r#"{"op":"count","from":{"file":{"at":"a","format":"xml"}}}"#,
                "a file is read as csv, csvText, parquet or json, not xml",
            ),
            (
                r#"{"op":"count","from":{"remote":{"url":"a","format":"json"}}}"#,
                "a remote file is csv, parquet or iceberg, not json",
            ),
            (r#"{"op":"count","from":{"query":null}}"#, "an operation's query is a string"),
            (
                r#"{"op":"relation","kind":"table","target":{"name":"t"},"replacing":null,"from":{"query":"q"}}"#,
                "a relation's replacing is an array",
            ),
            (
                r#"{"op":"relation","kind":"table","target":{"name":"t"},"replacing":[1],"from":{"query":"q"}}"#,
                "a conversion is a JSON object",
            ),
            (
                r#"{"op":"insert","target":{"name":"t"},"from":{"query":"q"}}"#,
                "rows put in name their columns, an array",
            ),
            (
                r#"{"op":"insert","target":{"name":"t"},"columns":[[]],"from":{"query":"q"}}"#,
                "a column is a JSON object",
            ),
            (
                r#"{"op":"secret","name":"s","scope":"x","credentials":{"KEY_ID":1}}"#,
                "a credential is a string: KEY_ID",
            ),
            (r#"{"op":"extensions","names":["a",1]}"#, "an operation's names is an array of strings"),
            (r#"{"op":"icebergManifests"}"#, "an operation's lists is an array of strings"),
        ];
        for (json, want) in cases {
            assert_eq!(refused(json), want, "{json}");
        }
    }

    #[test]
    fn a_database_without_host_operations_refuses_each_by_name() {
        let refused = Refused { database: "Postgres".to_owned() };
        let r = refused.open_database("p").unwrap_err();
        assert_eq!(r.class, Refusal::DIALECT_CAPABILITY);
        assert_eq!(
            r.message,
            "a project's own database on Postgres is not built: only DuckDB makes one so far \
             (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md C5)"
        );
        assert_eq!(refused.use_database("p").unwrap_err(), r);
        assert_eq!(refused.close_database("p").unwrap_err(), r);
        assert_eq!(
            refused.statements(&HostOperation::Platform).unwrap_err().message,
            "a host's Platform on Postgres is not built (docs/ONE_PLATFORM_PROGRAM_2026_10_12.md D5)"
        );
    }
}
