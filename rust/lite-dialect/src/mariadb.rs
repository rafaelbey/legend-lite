// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `MariaDb` (`core/src/main/java/com/legend/sql/dialect/MariaDb.java`), the MariaDB execution dialect, and
//! `MySql` (`MySql.java`, `extends MariaDb`), the MySQL one: the base renderer under the session's `ANSI_QUOTES` (every
//! identifier double-quoted), a quote doubled in a literal (a text with an odd run of backslashes before a quote
//! spelled around `CHAR(92 USING utf8mb4)`), a Float literal a DOUBLE by its exponent, NULLs placed by a key of their
//! own, no LATERAL, QUALIFY, VALUES alias list or FULL OUTER JOIN, and lists, structs, maps and Variants as JSON text
//! -- each list operation a correlated subquery over `JSON_TABLE`, a fold, a reduction and an edit distance a recursive
//! CTE over values that read no row, a MODE counted in a select over its source, a root select's values delivered as
//! its labels say, a raise kept in a session variable -- and, as its [`crate::SqlDialect`], its session statements, its
//! keys snapshot, its 4,096 result columns, a static pivot, its wire names and the conventions it delivers by. Every
//! `SqlFn` and aggregate is decided by an exhaustive match: `src/mariadb/functions.rs` (`call` and its helpers, the
//! lists, the maps, the regular expressions and the temporal helpers), `src/mariadb/json.rs` (how a list, a struct, a
//! map and a Variant hold a value, the casts and texts of a value, the folds and the walks),
//! `src/mariadb/aggregates.rs` (the hooks of the base, the aggregates and windows, the JSON constructors, the DDL and
//! the wire) and `src/mariadb/passes_family.rs` (`WindowAggregatesAsCorrelated` and MySQL's `WindowsComputedBelow`, the
//! two passes only this family installs).
//!
//! MySql is this struct with `mysql` set: Java's subclass overrides 28 methods, 19 of them MariaDb's own extension
//! points (`compact`, `arrayAgg`, `indexPath`, `groupRef`, `product`, `jsonTable`, both `jsonText`s, `jsonMember`,
//! `booleanColumn`, `jsonIsBinary`, `correlatesDerived`, `windowQuantilesNative`, `groupedModesCounted`, `jsonAsText`,
//! `twoRows`, `yearOf`, `format`, `regexpReplace`), and each is a branch on it in the one method that is Java's hook --
//! as are `passes`, `stringLit`, `hugeIntLiteral`, `sessionSetup` and `windowCall`. Its `scalarTypeName`, `formatText`,
//! `reducer` and `call` are MariaDb's by delegation (an exhaustive switch whose every arm calls `super`).
//!
//! Java's per-render state -- `laterals` (the explosion markers in scope of each select being written), `asText` (root
//! values written as their text, by identity), `roots` (the labels of the statement being rendered), `rootSelects` (its
//! root's selects, by identity, once its root is written) and `modeCounts` (each select's counted MODEs and quantile
//! ranks) -- is the dialect object's own, `RefCell` stacks pushed and popped where Java pushes and pops them; nothing
//! outlives a render. Java's identity maps are addresses here: a root select is the node `query` is handed (its address
//! is stable while the statement is written), a select made in its stead (a marker's reads, a counted MODE's select) is
//! one by being made from one, as Java's `put` makes it, and a root value is the projection's own node in the select
//! about to be written, forgotten once it is written (Java's `remove`).

use crate::ansi;
use crate::dialect::KeysSnapshot;
use crate::explosion_markers::{
    self, exploded_read, is_marker, is_marker_select, marker_column, null_list, unnest_list,
};
use crate::pieces::{Ex, P, pc, text};
use crate::refusal::{Refusal, arg};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon};
use crate::rewriter::SqlRewriter;
use crate::statement::{RenderedStatement, TypeSpelling};
use crate::typing::{self, source_alias, source_outputs};
use crate::writer::SqlWriter;
use crate::{build, carrier, passes, shared_passes};
use lite_sql::{
    AggFn, BigDecimal, ColumnType, DateFmt, Frame, JoinKind, JsonArrayAggKey, JsonKind, Lambda, NameQuoting, NullOrder,
    Origin, OutputCol, Projection, Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery,
    SqlSelect, SqlSource, SqlType, StructField, StructLitField, TypeFact, ValueKind, When,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::LazyLock;

mod aggregates;
mod functions;
mod json;
pub(crate) mod passes_family;

/// The explosion markers of one select in scope: each marker's alias and its exploded columns' lists, in order (Java's
/// `Map<String, Map<String, SqlExpr>>`, a `LinkedHashMap` of them).
type Scope = Vec<(String, Vec<(String, SqlExpr)>)>;

/// One select's counted MODEs and ranked quantiles: each one (a grouped reducer or a window call) and the columns
/// counting it (Java's `LinkedHashMap<SqlExpr, List<SqlExpr>>`, keyed by record equality).
type Counts = Vec<(SqlExpr, Vec<SqlExpr>)>;

/// The MariaDB dialect, or (with `mysql`) MySQL's: one per render, as Java's `new MariaDb()` and `new MySql()`.
#[derive(Debug, Default)]
pub struct MariaDb {
    ansi: AnsiState,
    /// Java's `MySql`: MariaDB's spellings where the two agree, its own hooks where not.
    mysql: bool,
    /// Java's `laterals`: the markers in scope of each select being written, the innermost last.
    laterals: RefCell<Vec<Scope>>,
    /// Java's `asText`: root values written as their text, by the address of the node about to be written.
    as_text: RefCell<Vec<usize>>,
    /// Java's `roots`: the labels of each statement being rendered (its root's outputs before the passes).
    roots: RefCell<Vec<Vec<OutputCol>>>,
    /// Java's `rootSelects`: per statement being rendered, its root's selects by address once its root is written
    /// (`None` while it is still to come: Java's empty map).
    root_selects: RefCell<Vec<Option<Vec<usize>>>>,
    /// Java's `modeCounts`: per select being written, each counted MODE in it and its columns.
    mode_counts: RefCell<Vec<Counts>>,
}

impl MariaDb {
    /// Java's `new MariaDb()`.
    pub fn new() -> MariaDb {
        MariaDb::default()
    }

    /// Java's `new MySql()`.
    pub fn mysql() -> MariaDb {
        MariaDb { mysql: true, ..MariaDb::default() }
    }

    /// Whether this is MySQL's dialect.
    pub fn is_mysql(&self) -> bool {
        self.mysql
    }
}

/// MariaDB's reserved words (Java's `MariaDb.LEXICON`'s, MySql's the same). The dialect quotes every identifier, so
/// the list decides no spelling.
pub const RESERVED: &[&str] = &[
    "accessible",
    "add",
    "all",
    "alter",
    "analyze",
    "and",
    "as",
    "asc",
    "asensitive",
    "before",
    "between",
    "bigint",
    "binary",
    "blob",
    "both",
    "by",
    "call",
    "cascade",
    "case",
    "change",
    "char",
    "character",
    "check",
    "collate",
    "column",
    "condition",
    "constraint",
    "continue",
    "convert",
    "create",
    "cross",
    "current_date",
    "current_role",
    "current_time",
    "current_timestamp",
    "current_user",
    "cursor",
    "database",
    "databases",
    "day_hour",
    "day_microsecond",
    "day_minute",
    "day_second",
    "dec",
    "decimal",
    "declare",
    "default",
    "delayed",
    "delete",
    "desc",
    "describe",
    "deterministic",
    "distinct",
    "distinctrow",
    "div",
    "double",
    "drop",
    "dual",
    "each",
    "else",
    "elseif",
    "enclosed",
    "escaped",
    "except",
    "exists",
    "exit",
    "explain",
    "false",
    "fetch",
    "float",
    "for",
    "force",
    "foreign",
    "from",
    "fulltext",
    "general",
    "grant",
    "group",
    "having",
    "high_priority",
    "if",
    "ignore",
    "in",
    "index",
    "infile",
    "inner",
    "inout",
    "insensitive",
    "insert",
    "int",
    "integer",
    "intersect",
    "interval",
    "into",
    "is",
    "iterate",
    "join",
    "key",
    "keys",
    "kill",
    "leading",
    "leave",
    "left",
    "like",
    "limit",
    "lines",
    "load",
    "localtime",
    "localtimestamp",
    "lock",
    "long",
    "loop",
    "match",
    "minus",
    "mod",
    "modifies",
    "natural",
    "not",
    "null",
    "numeric",
    "offset",
    "on",
    "optimize",
    "option",
    "or",
    "order",
    "out",
    "outer",
    "over",
    "partition",
    "precision",
    "primary",
    "procedure",
    "range",
    "read",
    "reads",
    "real",
    "recursive",
    "references",
    "regexp",
    "release",
    "rename",
    "repeat",
    "replace",
    "require",
    "restrict",
    "return",
    "returning",
    "revoke",
    "right",
    "rlike",
    "row_number",
    "rows",
    "schema",
    "schemas",
    "select",
    "sensitive",
    "separator",
    "set",
    "show",
    "signal",
    "smallint",
    "spatial",
    "specific",
    "sql",
    "starting",
    "straight_join",
    "table",
    "terminated",
    "then",
    "to",
    "trailing",
    "trigger",
    "true",
    "undo",
    "union",
    "unique",
    "unlock",
    "unsigned",
    "update",
    "usage",
    "use",
    "using",
    "values",
    "varchar",
    "when",
    "where",
    "while",
    "window",
    "with",
    "write",
    "xor",
    "zerofill",
];

/// Java's `MariaDb.LEXICON`: double quotes (the session's `ANSI_QUOTES`) and [`RESERVED`].
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', RESERVED));

/// Java's `MariaDb.CAPS`: the portable strategies over its JSON lists (a JSON_TABLE argument correlates); no FULL
/// OUTER JOIN, so it is emulated.
pub const CAPS: carrier::Caps =
    carrier::Caps { native_lists: false, correlated_explode: true, json_carrier: true, native_full_outer: false };

// ==================================================================
// The type facts the arms read
// ==================================================================

/// Java's `typeOf(e)`.
fn type_of(e: &SqlExpr) -> Option<&SqlType> {
    fact_type(e.r#type())
}

fn fact_type(f: &TypeFact) -> Option<&SqlType> {
    match f {
        TypeFact::Typed { r#type, .. } => Some(r#type),
        _ => None,
    }
}

/// Java's `listElement(e)`: the element type of a list, none for a value that is no list.
fn list_element(e: &SqlExpr) -> Option<&SqlType> {
    fact_element(e.r#type())
}

fn fact_element(f: &TypeFact) -> Option<&SqlType> {
    match fact_type(f) {
        Some(SqlType::Array { element }) => Some(element),
        _ => None,
    }
}

fn is_type(t: Option<&SqlType>, s: Scalar) -> bool {
    matches!(t, Some(SqlType::Scalar(x)) if *x == s)
}

/// Java's `nested(t)`: a list, a struct, a map or a Variant.
fn nested(t: Option<&SqlType>) -> bool {
    matches!(
        t,
        Some(SqlType::Array { .. } | SqlType::Struct { .. } | SqlType::Map { .. } | SqlType::Scalar(Scalar::Json))
    )
}

/// Java's `isJson(e)`.
fn is_json(e: &SqlExpr) -> bool {
    is_type(type_of(e), Scalar::Json)
}

/// Java's `isText(e)`: a string literal, or a VARCHAR.
fn is_text(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::StringLit { .. }) || is_type(type_of(e), Scalar::Varchar)
}

/// Java's `isDouble(e)`: a Float literal, or a DOUBLE.
fn is_double(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::FloatLit { .. }) || is_type(type_of(e), Scalar::Double)
}

/// Java's `composite(e)`: a struct, a list or a map.
fn composite(e: &SqlExpr) -> bool {
    matches!(type_of(e), Some(SqlType::Struct { .. } | SqlType::Array { .. } | SqlType::Map { .. }))
}

/// Java's `integral(e)`.
fn integral(e: &SqlExpr) -> bool {
    matches!(type_of(e), Some(SqlType::Scalar(Scalar::Bigint | Scalar::Integer | Scalar::Hugeint)))
}

/// A `SqlType`'s `toString`, or `null` for none (Java's string concatenation of a nullable type).
fn type_text(t: Option<&SqlType>) -> String {
    t.map_or_else(|| "null".to_owned(), crate::java::sql_type_text)
}

/// A source's class's simple name (`src.getClass().getSimpleName()`).
fn source_class(src: &SqlSource) -> &'static str {
    match src {
        SqlSource::Table { .. } => "Table",
        SqlSource::Subselect { .. } => "Subselect",
        SqlSource::Cte { .. } => "Cte",
        SqlSource::Join { .. } => "Join",
        SqlSource::Pivot { .. } => "Pivot",
        SqlSource::Values { .. } => "Values",
        SqlSource::Dual => "Dual",
        SqlSource::SourceUrl { .. } => "SourceUrl",
        SqlSource::RawSql { .. } => "RawSql",
        SqlSource::Catalog { .. } => "Catalog",
        SqlSource::VarSetPlaceholder { .. } => "VarSetPlaceholder",
    }
}

/// A projection's output name: its alias, else a bare column's name, else a stored read's column's (Java's
/// `Projection.outputName()`).
fn output_name(p: &Projection) -> Option<String> {
    match (&p.alias, &p.expr) {
        (Some(a), _) => Some(a.clone()),
        (None, SqlExpr::Column(c)) => Some(c.name.clone()),
        (None, SqlExpr::StoredRead { column, .. }) => Some(column.name.clone()),
        _ => None,
    }
}

// ==================================================================
// Lambdas
// ==================================================================

/// The position alias of a lambda's element (Java's `offsetOf`): unique per lambda.
fn offset_of(l: &Lambda) -> String {
    format!("{}__o", l.params[0])
}

/// A lambda's index parameter (Java's `indexOf`).
fn index_of(l: &Lambda) -> Option<&str> {
    l.params.get(1).map(String::as_str)
}

/// A lambda's body over elements of `element` type (Java's `lambdaBody`): a field read off a struct parameter becomes a
/// field read of the JSON object, for every parameter.
fn lambda_body(l: &Lambda, element: Option<&SqlType>) -> Result<SqlExpr, Refusal> {
    let mut body = l.body.as_ref().clone();
    for (i, p) in l.params.iter().enumerate() {
        if let Some(b) = struct_fields(&body, p, if i == 0 { element } else { None })? {
            body = b;
        }
    }
    Ok(body)
}

/// Java's `structFields`: `None` where nothing changed (Java's identity).
fn struct_fields(body: &SqlExpr, param: &str, r#struct: Option<&SqlType>) -> Result<Option<SqlExpr>, Refusal> {
    if let SqlExpr::Column(c) = body
        && c.table.as_deref() == Some(param)
    {
        // the field read typed as the column it replaces where the parameter's struct is not known here
        let source = match r#struct {
            Some(st @ SqlType::Struct { .. }) => build::typed_column(None, param, st.clone(), true, Origin::Derived),
            _ => build::derived(None, param),
        };
        return Ok(Some(build::struct_get_given(source, c.name.clone(), c.r#type.clone())));
    }
    carrier::map_children(body, &mut |k| struct_fields(k, param, r#struct))
}

/// Java's `closed(e, bound)`: whether `e` reads no column but the bound names, and holds no subquery.
fn closed(e: &SqlExpr, bound: &HashSet<String>) -> bool {
    if let SqlExpr::Lambda(l) = e {
        let mut inner = bound.clone();
        inner.extend(l.params.iter().cloned());
        return closed(&l.body, &inner);
    }
    if let SqlExpr::Column(c) = e {
        return bound.contains(c.table.as_deref().unwrap_or(&c.name));
    }
    if matches!(
        e,
        SqlExpr::ScalarSubquery { .. }
            | SqlExpr::Exists { .. }
            | SqlExpr::InSubquery { .. }
            | SqlExpr::Quantified { .. }
    ) {
        return false;
    }
    typing::children(e).iter().all(|k| closed(k, bound))
}

/// `closed(e, Set.of())`.
fn closed_alone(e: &SqlExpr) -> bool {
    closed(e, &HashSet::new())
}

// ==================================================================
// The family's hooks: where MySQL spells it otherwise
// ==================================================================

impl MariaDb {
    /// Java's `compact`: a JSON text as a JSON value, normalized -- `JSON_COMPACT` (MySQL: its CAST to JSON).
    fn compact<'a>(&self, json: P<'a>) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("CAST(").piece(&*json)?.append(" AS JSON)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("JSON_COMPACT(").piece(&*json)?.append(")");
            Ok(())
        })
    }

    /// Java's `arrayAgg`: `JSON_ARRAYAGG([DISTINCT] value [ORDER BY order] [LIMIT 1])` (MySQL: an ordered or distinct
    /// one is GROUP_CONCAT's text of each value's one-element array, read as JSON and unwrapped).
    fn array_agg<'a>(&self, distinct: bool, value: P<'a>, order: Option<P<'a>>, first_only: bool) -> P<'a> {
        if self.mysql {
            if order.is_none() && !distinct {
                return pc(move |w| {
                    w.append("JSON_ARRAYAGG(").piece(&*value)?.append(")");
                    Ok(())
                });
            }
            return pc(move |w| {
                w.append("JSON_EXTRACT(CAST(CONCAT('[', GROUP_CONCAT(").append(if distinct { "DISTINCT " } else { "" });
                w.append("JSON_ARRAY(").piece(&*value)?.append(")");
                if let Some(order) = &order {
                    w.append(" ORDER BY ").piece(&**order)?;
                }
                w.append(" SEPARATOR ','), ']') AS JSON), '$[*][0]')");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("JSON_ARRAYAGG(").append(if distinct { "DISTINCT " } else { "" }).piece(&*value)?;
            if let Some(order) = &order {
                w.append(" ORDER BY ").piece(&**order)?;
                if first_only {
                    w.append(" LIMIT 1");
                }
            }
            w.append(")");
            Ok(())
        })
    }

    /// Java's `indexPath`: `'$[' || index || ']'` (MySQL: no negative index, `$[last-n]`).
    fn index_path<'a>(&self, index: P<'a>) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("(CASE WHEN (").piece(&*index)?.append(") < 0 THEN CONCAT('$[last-', -(").piece(&*index)?;
                w.append(") - 1, ']') ELSE CONCAT('$[', ").piece(&*index)?.append(", ']') END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("CONCAT('$[', ").piece(&*index)?.append(", ']')");
            Ok(())
        })
    }

    /// Java's `groupRef`: a replacement's reference to group `n` (PCRE's `\n`; MySQL's ICU `$n`).
    fn group_ref(&self, n: i64) -> String {
        if self.mysql { format!("${n}") } else { format!("\\{n}") }
    }

    /// Java's `product()`: the database's name, as an error names it.
    fn product(&self) -> &'static str {
        if self.mysql { "MySQL" } else { "MariaDB" }
    }

    /// Java's `jsonTable`: `JSON_TABLE(json, spec) AS alias` (MySQL: the argument bound first as the one column of a
    /// LATERAL derived table, which the table function reads).
    fn json_table<'a>(&self, json: P<'a>, spec: String, alias: String) -> P<'a> {
        if self.mysql {
            let bound = self.ident(&format!("{}__j", alias.replace('"', "")));
            return pc(move |w| {
                w.append("(LATERAL (SELECT ").piece(&*json)?;
                w.append(&format!(" AS \"j\") AS {bound} CROSS JOIN JSON_TABLE({bound}.\"j\", {spec}) AS {alias})"));
                Ok(())
            });
        }
        pc(move |w| {
            w.append("JSON_TABLE(").piece(&*json)?.append(&format!(", {spec}) AS {alias}"));
            Ok(())
        })
    }

    /// Java's `jsonText(Piece, Piece)`: a JSON scalar's SQL value at `path` -- `JSON_VALUE` (MySQL: `JSON_EXTRACT`'s,
    /// unquoted, a JSON null NULL).
    fn json_text<'a>(&self, json: P<'a>, path: P<'a>) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("(CASE WHEN JSON_TYPE(JSON_EXTRACT(").piece(&*json)?.append(", ").piece(&*path)?;
                w.append(")) = 'NULL' THEN NULL ELSE JSON_UNQUOTE(JSON_EXTRACT(").piece(&*json)?.append(", ");
                w.piece(&*path)?.append(")) END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("JSON_VALUE(").piece(&*json)?.append(", ").piece(&*path)?.append(")");
            Ok(())
        })
    }

    /// Java's `jsonText(String, String)`: [`MariaDb::json_text`] of SQL text.
    fn json_text_of(&self, json: &str, path: &str) -> String {
        if self.mysql {
            return format!(
                "(CASE WHEN JSON_TYPE(JSON_EXTRACT({json}, {path})) = 'NULL' THEN NULL ELSE JSON_UNQUOTE(JSON_EXTRACT(\
                 {json}, {path})) END)"
            );
        }
        format!("JSON_VALUE({json}, {path})")
    }

    /// Java's `jsonMember`: a list, struct or map at `path`, as its JSON, a JSON null NULL.
    fn json_member<'a>(&self, json: P<'a>, path: P<'a>) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("(CASE WHEN JSON_TYPE(JSON_EXTRACT(").piece(&*json)?.append(", ").piece(&*path)?;
                w.append(")) = 'NULL' THEN NULL ELSE JSON_EXTRACT(").piece(&*json)?.append(", ").piece(&*path)?;
                w.append(") END)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("NULLIF(JSON_EXTRACT(").piece(&*json)?.append(", ").piece(&*path)?.append("), 'null')");
            Ok(())
        })
    }

    /// Java's `booleanColumn`: a JSON_TABLE column of booleans (MySQL's is an INT: its BOOLEAN is a BIT there).
    fn boolean_column(&self) -> &'static str {
        if self.mysql { "INT" } else { "BOOLEAN" }
    }

    /// Java's `jsonIsBinary`: whether a JSON value reaches the driver as bytes (MySQL's does).
    fn json_is_binary(&self) -> bool {
        self.mysql
    }

    /// Java's `correlatesDerived`: whether a recursive CTE or a derived table may read an outer row (MySQL's may).
    fn correlates_derived(&self) -> bool {
        self.mysql
    }

    /// Java's `windowQuantilesNative`: whether a whole partition's quantile is a native window function (not MySQL's).
    fn window_quantiles_native(&self) -> bool {
        !self.mysql
    }

    /// Java's `groupedModesCounted`: whether a grouped mode is counted too (MySQL's is).
    fn grouped_modes_counted(&self) -> bool {
        self.mysql
    }

    /// Java's `jsonAsText`: a JSON value as its text, compact (MySQL: its spaces after `,` and `:` outside a string
    /// dropped).
    fn json_as_text<'a>(&self, json: P<'a>) -> P<'a> {
        if self.mysql {
            let pattern = self.string_literal("(\"(?:[^\"\\\\]|\\\\.)*\")|([,:]) ");
            return pc(move |w| {
                w.append("REGEXP_REPLACE(CAST(").piece(&*json)?.append(" AS CHAR), ").append(&pattern);
                w.append(", '$1$2')");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("CAST(").piece(&*json)?.append(" AS CHAR)");
            Ok(())
        })
    }

    /// Java's `twoRows`: a scalar subquery of two rows that keeps `message` in `@legend_raise` as it runs (MySQL: a
    /// UNION's, its first assigning).
    fn two_rows<'a>(&self, message: P<'a>) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("(SELECT @legend_raise := ").piece(&*message)?.append(" UNION ALL SELECT 0)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("(SELECT 1 FROM JSON_TABLE(JSON_ARRAY(@legend_raise := ").piece(&*message)?;
            w.append(", 0), '$[*]' COLUMNS (\"x\" LONGTEXT PATH '$')) AS __r)");
            Ok(())
        })
    }

    /// Java's `yearOf`: a date's year, an integer (MySQL: cast from its YEAR type).
    fn year_of<'a>(&'a self, t: &'a SqlExpr) -> P<'a> {
        if self.mysql {
            return pc(move |w| {
                w.append("CAST(YEAR(").expr(self, t, 0)?.append(") AS SIGNED)");
                Ok(())
            });
        }
        pc(move |w| {
            w.append("YEAR(").expr(self, t, 0)?.append(")");
            Ok(())
        })
    }

    /// Java's `stringLit` (MariaDb's and MySql's): never refused.
    fn string_literal(&self, value: &str) -> String {
        if self.mysql { mysql_string_lit(value) } else { mariadb_string_lit(value) }
    }
}

/// MariaDB's string literal (Java's `MariaDb.stringLit`): a quote doubled; a text with an odd run of backslashes
/// before a quote spells each backslash `CHAR(92 USING utf8mb4)`.
fn mariadb_string_lit(value: &str) -> String {
    let body = value.replace('\'', "''");
    if !odd_backslashes_before_quote(&format!("{body}'")) {
        return format!("'{body}'");
    }
    let mut sb = String::from("CONCAT(");
    for (i, part) in value.split('\\').enumerate() {
        sb.push_str(if i > 0 { ", CHAR(92 USING utf8mb4), " } else { "" });
        sb.push('\'');
        sb.push_str(&part.replace('\'', "''"));
        sb.push('\'');
    }
    sb.push(')');
    sb
}

/// Java's `oddBackslashesBeforeQuote`.
fn odd_backslashes_before_quote(text: &str) -> bool {
    let mut run = 0usize;
    for ch in text.chars() {
        if ch == '\\' {
            run += 1;
        } else {
            if ch == '\'' && run % 2 == 1 {
                return true;
            }
            run = 0;
        }
    }
    false
}

/// MySQL's string literal (Java's `MySql.stringLit`): no quote and no backslash inside a literal -- each is its own
/// character, `CHAR(39 USING utf8mb4)` and `CHAR(92 USING utf8mb4)`, between literal pieces.
fn mysql_string_lit(value: &str) -> String {
    if !value.contains('\'') && !value.contains('\\') {
        return format!("'{value}'");
    }
    let mut sb = String::from("CONCAT(''");
    let mut piece = String::new();
    for ch in value.chars() {
        if ch == '\'' || ch == '\\' {
            if !piece.is_empty() {
                sb.push_str(", '");
                sb.push_str(&piece);
                sb.push('\'');
                piece.clear();
            }
            sb.push_str(if ch == '\'' { ", CHAR(39 USING utf8mb4)" } else { ", CHAR(92 USING utf8mb4)" });
        } else {
            piece.push(ch);
        }
    }
    if !piece.is_empty() {
        sb.push_str(", '");
        sb.push_str(&piece);
        sb.push('\'');
    }
    sb.push(')');
    sb
}

// ==================================================================
// Structure: the statement's root, its selects, the markers, the lateral rows
// ==================================================================

/// Each select of a root (the root, a union's branches, a WITH's body) by address (Java's `selectsOf`).
fn selects_of(q: &SqlQuery) -> Vec<usize> {
    let mut out = Vec::new();
    collect_selects(q, &mut out);
    out
}

fn collect_selects(q: &SqlQuery, into: &mut Vec<usize>) {
    match q {
        SqlQuery::Select(s) => into.push(address_of_select(s)),
        SqlQuery::Union(u) => u.branches.iter().for_each(|b| collect_selects(b, into)),
        SqlQuery::With(w) => collect_selects(&w.body, into),
    }
}

fn address_of_select(s: &SqlSelect) -> usize {
    s as *const SqlSelect as usize
}

fn address_of(e: &SqlExpr) -> usize {
    e as *const SqlExpr as usize
}

/// `LinkedHashMap.put`: a key met again keeps its place, its value replaced.
fn put<V>(into: &mut Vec<(SqlExpr, V)>, key: SqlExpr, value: V) {
    match into.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => into.push((key, value)),
    }
}

/// A MODE or a ranked quantile where it is written: a grouped reducer, or a window call over one (Java's `SqlExpr
/// mode`, a key of `modeCounts`).
#[derive(Clone, Copy)]
enum Counted<'a> {
    Grouped(&'a Reducer),
    Window { r#fn: &'a SqlAgg, partition_by: &'a [SqlExpr], order_by: &'a [SortKey], frame: Option<&'a Frame> },
}

impl Counted<'_> {
    /// Whether `key` (a MODE `modesCounted` recorded) is this one, by Java's record equality (a window's type fact is
    /// its function's, so its parts decide).
    fn is(&self, key: &SqlExpr) -> bool {
        match (self, key) {
            (Counted::Grouped(r), SqlExpr::Reducer(k)) => *r == k,
            (
                Counted::Window { r#fn, partition_by, order_by, frame },
                SqlExpr::WindowCall { r#fn: kf, partition_by: kp, order_by: ko, frame: kr, .. },
            ) => {
                **r#fn == **kf && *partition_by == kp.as_slice() && *order_by == ko.as_slice() && *frame == kr.as_ref()
            }
            _ => false,
        }
    }

    /// The reducer it is (Java's `(SqlAgg.Reducer) w.fn()` or `(SqlAgg.Reducer) mode`).
    fn reducer(&self) -> &Reducer {
        match self {
            Counted::Grouped(r) => r,
            Counted::Window { r#fn, .. } => match r#fn {
                SqlAgg::Reducer(r) => r,
                _ => unreachable!("a counted window is a reducer's"),
            },
        }
    }
}

/// The reducer a recorded MODE is (Java's cast of the key).
fn key_reducer(key: &SqlExpr) -> &Reducer {
    match key {
        SqlExpr::WindowCall { r#fn, .. } => match r#fn.as_ref() {
            SqlAgg::Reducer(r) => r,
            _ => unreachable!("a counted window is a reducer's"),
        },
        SqlExpr::Reducer(r) => r,
        _ => unreachable!("a counted MODE is a reducer or a window call"),
    }
}

impl MariaDb {
    /// Java's `pendingRoot()`: the statement's root is still to come.
    fn pending_root(&self) -> bool {
        matches!(self.root_selects.borrow().last(), Some(None))
    }

    /// Whether `s` is one of the statement's root selects (Java's `top.containsKey(s)`, its value TRUE).
    fn is_root(&self, s: &SqlSelect) -> bool {
        let address = address_of_select(s);
        matches!(self.root_selects.borrow().last(), Some(Some(selects)) if selects.contains(&address))
    }

    /// Java's `modeValue`: the value a mode counts.
    fn mode_value<'e>(&self, r: &'e Reducer) -> Result<&'e SqlExpr, Refusal> {
        if r.args.len() != 1 || r.distinct {
            return Err(Refusal::capability(format!(
                "a mode of {} arguments reached {}",
                r.args.len(),
                self.product()
            )));
        }
        Ok(&r.args[0])
    }

    /// The columns counting `mode` in the select being written (Java's `modeCounts.peek().get(mode)`).
    fn counted(&self, mode: Counted<'_>) -> Option<Vec<SqlExpr>> {
        let counts = self.mode_counts.borrow();
        counts.last().and_then(|top| top.iter().find(|(k, _)| mode.is(k)).map(|(_, c)| c.clone()))
    }

    /// Java's `modesCounted`: `s`, its FROM given a count per MODE (and a rank and a count per quantile where they are
    /// read from the ranks).
    fn modes_counted(&self, s: &SqlSelect, counts: &mut Counts) -> Result<Option<SqlSelect>, Refusal> {
        let mut modes: Counts = Vec::new();
        let mut roots: Vec<&SqlExpr> = s.projections.iter().map(|p| &p.expr).collect();
        roots.extend(s.having.as_ref());
        roots.extend(s.qualify.as_ref());
        for e in roots {
            self.collect_modes(e, &s.group_by, &mut modes);
        }
        if modes.is_empty() {
            return Ok(None);
        }
        let from = &s.from;
        let alias = source_alias(from)?.to_owned();
        let outputs = source_outputs(from);
        if matches!(from, SqlSource::Join { .. }) || outputs.is_empty() {
            return Err(Refusal::capability(format!(
                "a MODE over a {} reached {}: its counts are a select over one aliased source",
                source_class(from),
                self.product()
            )));
        }
        let mut ps = Vec::new();
        for oc in outputs {
            let read = SqlExpr::Column(build::column_of(Some(&alias), oc));
            ps.push(build::projection(read, Some(oc.name.clone()), Some(oc.clone()))?);
        }
        for (k, (mode, partition_by)) in modes.into_iter().enumerate() {
            let name = format!("__mc{k}");
            let r = key_reducer(&mode);
            if r.r#fn != AggFn::Mode {
                // a quantile: the row's rank by its value in its partition (NULLs last, the quantile's direction), and
                // the partition's count of values
                let v = arg(&r.args, 0)?.clone();
                let desc = r.order_by.first().is_some_and(|k| !k.ascending);
                let rank = build::window_call(
                    SqlAgg::RankingFn { r#fn: AggFn::RowNumber, args: Vec::new() },
                    partition_by.clone(),
                    vec![build::sort_key(v.clone(), !desc, Some(NullOrder::NullsLast), None)],
                    None,
                );
                ps.push(build::projection(rank, Some(format!("{name}r")), None)?);
                let count = build::window_call(
                    SqlAgg::Reducer(build::reducer(AggFn::Count, vec![v], false, Vec::new())),
                    partition_by,
                    Vec::new(),
                    None,
                );
                ps.push(build::projection(count, Some(format!("{name}n")), None)?);
                let columns = vec![
                    build::derived(Some(&alias), &format!("{name}r")),
                    build::derived(Some(&alias), &format!("{name}n")),
                ];
                put(counts, mode, columns);
                continue;
            }
            let mut partition = partition_by;
            partition.push(self.mode_value(r)?.clone());
            let value = self.mode_value(r)?.clone();
            let count = build::window_call(
                SqlAgg::Reducer(build::reducer(AggFn::Count, vec![value], false, Vec::new())),
                partition,
                Vec::new(),
                None,
            );
            ps.push(build::projection(count, Some(name.clone()), None)?);
            put(counts, mode, vec![build::derived(Some(&alias), &name)]);
        }
        let counted_rows = build::select_of(ps, from.clone(), Vec::new())?;
        // a star of the select reads the source as it was, its own columns, never the counts beside them
        let spelled = ansi::stars_as_source_columns(s, outputs, &alias, self.product())?;
        let s = build::with_projections(s.clone(), spelled)?;
        Ok(Some(build::with_from(s, build::subselect(build::query(counted_rows), &alias, None))?))
    }

    /// Java's `collectModes`: each MODE under `e` to count -- a grouped reducer with the select's keys (where grouped
    /// modes are counted), an unordered, unframed window call with its partition -- and, where quantiles are read from
    /// ranks, each unordered, unframed windowed quantile.
    fn collect_modes(&self, e: &SqlExpr, group_by: &[SqlExpr], out: &mut Counts) {
        if let SqlExpr::Reducer(r) = e
            && r.r#fn == AggFn::Mode
        {
            if self.grouped_modes_counted() {
                put(out, e.clone(), group_by.to_vec());
            }
            return;
        }
        if let SqlExpr::WindowCall { r#fn, partition_by, order_by, frame, .. } = e
            && let SqlAgg::Reducer(r) = r#fn.as_ref()
        {
            if r.r#fn == AggFn::Mode {
                if order_by.is_empty() && frame.is_none() {
                    put(out, e.clone(), partition_by.clone());
                }
                return;
            }
            if !self.window_quantiles_native()
                && matches!(r.r#fn, AggFn::Median | AggFn::QuantileCont | AggFn::QuantileDisc)
                && order_by.is_empty()
                && frame.is_none()
            {
                put(out, e.clone(), partition_by.clone());
                return;
            }
        }
        for c in typing::children(e) {
            self.collect_modes(&c, group_by, out);
        }
    }

    /// Java's `modeArm`: a counted mode where it is written -- the value of its most counted row, the least value on a
    /// tie.
    fn mode_arm(&self, w: &mut SqlWriter, mode: Counted<'_>) -> Result<(), Refusal> {
        let Some(counted) = self.counted(mode) else {
            return Err(Refusal::illegal_state(format!(
                "a MODE reached {}'s writer uncounted: modesCounted did not run",
                self.product()
            )));
        };
        let count = counted[0].clone();
        let v = self.mode_value(mode.reducer())?;
        if let Counted::Window { partition_by, .. } = mode {
            w.append("FIRST_VALUE(").expr(self, v, 0)?.append(") OVER (");
            if !partition_by.is_empty() {
                w.append("PARTITION BY ").list(self, partition_by)?.append(" ");
            }
            w.append("ORDER BY ").expr(self, &count, 0)?.append(" DESC, ").expr(self, v, 0)?;
            w.append(" ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)");
            return Ok(());
        }
        let order = pc(|o| {
            o.expr(self, &count, 0)?.append(" DESC, ").expr(self, v, 0)?;
            Ok(())
        });
        let first = self.array_agg(false, self.json(v), Some(order), true);
        let at = self.value_at(first, text("'$[0]'"), type_of(v));
        w.piece(&*at)?;
        Ok(())
    }

    /// Java's `delivered`: a root select's values as its labels deliver them -- a Float a DOUBLE, a value that claims
    /// no type its label's type -- a sort key naming a moved column its original expression. Answers the select and the
    /// positions of the values written as their text (Java's `asText`, MySQL's).
    fn delivered(&self, s: SqlSelect, rooted: bool) -> Result<(SqlSelect, Vec<usize>), Refusal> {
        let has_top = self.root_selects.borrow().last().is_some();
        let labels = self.roots.borrow().last().cloned();
        let Some(labels) = labels.filter(|l| has_top && rooted && l.len() == s.projections.len()) else {
            return Ok((s, Vec::new()));
        };
        let mut ps = Vec::with_capacity(s.projections.len());
        let mut moved: Vec<(String, SqlExpr)> = Vec::new();
        let mut as_text = Vec::new();
        for (i, p) in s.projections.iter().enumerate() {
            let label = &labels[i].r#type;
            let value = delivered_value(&p.expr, label);
            if self.json_is_binary()
                && nested(Some(label))
                && !matches!(
                    value.as_ref().unwrap_or(&p.expr),
                    SqlExpr::Star { .. } | SqlExpr::StarExcept { .. } | SqlExpr::NullLit { .. }
                )
            {
                as_text.push(i);
            }
            match value {
                None => ps.push(p.clone()),
                Some(value) => {
                    let name = output_name(p);
                    if let Some(name) = &name {
                        match moved.iter_mut().find(|(k, _)| k == name) {
                            Some(slot) => slot.1 = p.expr.clone(),
                            None => moved.push((name.clone(), p.expr.clone())),
                        }
                    }
                    ps.push(build::projection(value, name, p.out.clone())?);
                }
            }
        }
        if moved.is_empty() {
            return Ok((s, as_text));
        }
        let mut order = Vec::with_capacity(s.order_by.len());
        for k in &s.order_by {
            let original = match &k.expr {
                SqlExpr::Column(c) if c.table.is_none() => moved.iter().find(|(name, _)| *name == c.name),
                _ => None,
            };
            match original {
                Some((_, original)) => {
                    order.push(build::sort_key(original.clone(), k.ascending, k.null_order, k.output_name.clone()))
                }
                None => order.push(k.clone()),
            }
        }
        let s = build::select(SqlSelect { projections: ps, order_by: order, ..s })?;
        Ok((s, as_text))
    }

    /// Java's `explodedColumn`: an exploded column's list and the marker whose position reads it, cloned out of the
    /// scopes so nothing is borrowed while it is written.
    fn exploded_column(&self, c: &lite_sql::Column) -> Option<(SqlExpr, String)> {
        let laterals = self.laterals.borrow();
        for scope in laterals.iter().rev() {
            match &c.table {
                Some(t) => {
                    if let Some((_, cols)) = scope.iter().find(|(alias, _)| alias == t)
                        && let Some((_, xs)) = cols.iter().find(|(name, _)| *name == c.name)
                    {
                        return Some((xs.clone(), t.clone()));
                    }
                }
                None => {
                    let owners: Vec<&(String, Vec<(String, SqlExpr)>)> =
                        scope.iter().filter(|(_, cols)| cols.iter().any(|(name, _)| *name == c.name)).collect();
                    if owners.len() == 1 {
                        let (alias, cols) = owners[0];
                        let xs = &cols.iter().find(|(name, _)| *name == c.name).expect("an owner").1;
                        return Some((xs.clone(), alias.clone()));
                    }
                }
            }
        }
        None
    }

    /// Java's `element`: `xs[position]`, the list's element at the marker's 0-based position (NULL past its end).
    fn element(&self, w: &mut SqlWriter, xs: &SqlExpr, position: &str) -> Result<(), Refusal> {
        if null_list(xs) {
            w.append("NULL");
            return Ok(());
        }
        let index = text(format!("({}.\"key\" - 1)", self.alias_ident(position)));
        let at = self.element_at(xs, index);
        w.piece(&*at)?;
        Ok(())
    }

    /// Java's `positions`: `JSON_TABLE(<longest list>, '$[*]' COLUMNS ("key" FOR ORDINALITY)) AS alias`.
    fn positions<'a>(&'a self, m: &'a SqlSelect, alias: String) -> Result<P<'a>, Refusal> {
        let mut lists: Vec<&'a SqlExpr> = Vec::new();
        for p in &m.projections {
            let xs = unnest_list(&p.expr)?;
            if !null_list(xs) {
                lists.push(xs);
            }
        }
        let json = pc(move |w| {
            match lists.len() {
                0 => {
                    w.append("NULL");
                }
                1 => {
                    w.expr(self, lists[0], 0)?;
                }
                n => {
                    // the longest list (its positions are every list's)
                    w.append("CASE");
                    for i in 0..n - 1 {
                        let xs = lists[i];
                        w.append(" WHEN ");
                        for (j, other) in lists.iter().enumerate() {
                            if j != i {
                                w.append(if j > usize::from(i == 0) { " AND " } else { "" });
                                w.append("coalesce(JSON_LENGTH(").expr(self, xs, 0)?;
                                w.append("), 0) >= coalesce(JSON_LENGTH(").expr(self, other, 0)?.append("), 0)");
                            }
                        }
                        w.append(" THEN ").expr(self, xs, 0)?;
                    }
                    w.append(" ELSE ").expr(self, lists[n - 1], 0)?.append(" END");
                }
            }
            Ok(())
        });
        Ok(self.json_table(json, "'$[*]' COLUMNS (\"key\" FOR ORDINALITY)".to_owned(), alias))
    }

    /// Java's `lateralRows`: a lateral subquery's rows as a JSON array of objects, read back by `JSON_TABLE` as typed
    /// columns; a plain select written inline, any other a derived table.
    fn lateral_rows<'a>(&'a self, inner: &'a SqlQuery, depth: usize) -> Result<P<'a>, Refusal> {
        let outs = typing::query_outputs(inner);
        if outs.is_empty() {
            return Err(Refusal::capability(format!(
                "a lateral subquery of undeclared columns reached {}",
                self.product()
            )));
        }
        Ok(pc(move |w| {
            w.append("JSON_TABLE(coalesce((SELECT JSON_ARRAYAGG(JSON_OBJECT(");
            match inner {
                SqlQuery::Select(s) if inlineable(s) => {
                    let mut scope = Vec::new();
                    laterals_of(&s.from, &mut scope)?;
                    self.laterals.borrow_mut().push(scope);
                    let written = self.lateral_inline(w, s, outs, depth);
                    self.laterals.borrow_mut().pop();
                    written?;
                }
                _ => {
                    for (i, oc) in outs.iter().enumerate() {
                        w.append(if i > 0 { ", " } else { "" }).append(&self.string_literal(&oc.name)).append(", ");
                        let read = self.column_ref(&build::column_of(Some("__l"), oc))?;
                        let v = self.json_of(text(read), Some(&oc.r#type));
                        w.piece(&*v)?;
                    }
                    w.append(")) FROM (");
                    self.nl(w, depth + 1);
                    self.query(w, inner, depth + 1)?;
                    self.nl(w, depth);
                    w.append(") AS __l");
                }
            }
            w.append("), JSON_ARRAY()), '$[*]' COLUMNS (");
            for (i, oc) in outs.iter().enumerate() {
                let column = format!(
                    "{}{} {} PATH {}",
                    if i > 0 { ", " } else { "" },
                    self.alias_ident(&oc.name),
                    self.column_type(Some(&oc.r#type)),
                    self.member_path(&oc.name)
                );
                w.append(&column);
            }
            w.append("))");
            Ok(())
        }))
    }

    /// The inline arm of [`MariaDb::lateral_rows`]: the select's values aggregated in place, its FROM and WHERE under
    /// the aggregate.
    fn lateral_inline(
        &self,
        w: &mut SqlWriter,
        s: &SqlSelect,
        outs: &[OutputCol],
        depth: usize,
    ) -> Result<(), Refusal> {
        let values = self.lateral_values(s, outs)?;
        for (i, oc) in outs.iter().enumerate() {
            let v = &values[i];
            w.append(if i > 0 { ", " } else { "" }).append(&self.string_literal(&oc.name)).append(", ");
            let json = self.json_of(self.ex(v, 0), Some(&oc.r#type));
            w.piece(&*json)?;
        }
        w.append(")");
        if !s.order_by.is_empty() {
            w.append(" ORDER BY ").join(&s.order_by, ", ", |w, k| self.sort_key(w, k))?;
        }
        w.append(") FROM ");
        self.source(w, &s.from, depth + 1)?;
        if let Some(wh) = &s.r#where {
            w.append(" WHERE ").expr(self, wh, 0)?;
        }
        Ok(())
    }

    /// Java's `lateralValues`: each output's value in `s` -- its projection's, or a star's column.
    fn lateral_values(&self, s: &SqlSelect, outs: &[OutputCol]) -> Result<Vec<SqlExpr>, Refusal> {
        if s.projections.is_empty() {
            let table = if matches!(s.from, SqlSource::Join { .. }) { None } else { Some(source_alias(&s.from)?) };
            return Ok(outs.iter().map(|oc| SqlExpr::Column(build::column_of(table, oc))).collect());
        }
        let aligned =
            s.projections.len() == outs.len() && s.projections.iter().all(|p| !matches!(p.expr, SqlExpr::Star { .. }));
        if aligned {
            return Ok(s.projections.iter().map(|p| p.expr.clone()).collect());
        }
        if s.projections.len() == 1
            && let SqlExpr::Star { table, .. } = &s.projections[0].expr
        {
            return Ok(outs.iter().map(|oc| SqlExpr::Column(build::column_of(table.as_deref(), oc))).collect());
        }
        Err(Refusal::capability(format!(
            "a lateral subquery whose projections do not match its outputs reached {}",
            self.product()
        )))
    }
}

/// Java's `deliveredValue`: `None` where the value is delivered as it is.
fn delivered_value(e: &SqlExpr, label: &SqlType) -> Option<SqlExpr> {
    if matches!(e, SqlExpr::NullLit { .. } | SqlExpr::Star { .. } | SqlExpr::StarExcept { .. }) {
        return None;
    }
    let t = type_of(e);
    if *label == SqlType::Scalar(Scalar::Double) && !is_type(t, Scalar::Double) {
        return Some(build::cast(e.clone(), SqlType::Scalar(Scalar::Double)));
    }
    // a value that claims no type (an empty list's element) is its label's type
    if t.is_none()
        && matches!(
            label,
            SqlType::Decimal { .. }
                | SqlType::Scalar(
                    Scalar::Bigint
                        | Scalar::Integer
                        | Scalar::Hugeint
                        | Scalar::Boolean
                        | Scalar::Date
                        | Scalar::Timestamp
                        | Scalar::Varchar
                )
        )
    {
        return Some(build::cast(e.clone(), label.clone()));
    }
    None
}

/// Java's `starsSpelled`: `s` with each star over a FROM holding a marker spelled out.
fn stars_spelled(s: SqlSelect) -> Result<SqlSelect, Refusal> {
    let mut ps = Vec::new();
    if s.projections.is_empty() {
        spell(&s.from, None, &mut ps)?;
    }
    for p in &s.projections {
        if let SqlExpr::Star { table, .. } = &p.expr {
            spell(&s.from, table.as_deref(), &mut ps)?;
        } else {
            ps.push(p.clone());
        }
    }
    build::select(SqlSelect { projections: ps, ..s })
}

/// Java's `spell`: each other source's columns of a star, then each exploded column.
fn spell(src: &SqlSource, table: Option<&str>, into: &mut Vec<Projection>) -> Result<(), Refusal> {
    match src {
        SqlSource::Join { left, right, .. } => {
            spell(left, table, into)?;
            spell(right, table, into)
        }
        SqlSource::Subselect { inner, alias, .. } if is_marker(inner) && table.is_none_or(|t| t == alias) => {
            let SqlQuery::Select(m) = inner.as_ref() else { unreachable!("a marker is a select") };
            for p in &m.projections {
                let read = exploded_read(alias, p, p.out.as_ref().map(|o| &o.r#type))?;
                into.push(build::projection(read, Some(marker_column(p)?), p.out.clone())?);
            }
            Ok(())
        }
        SqlSource::Dual => Ok(()),
        other => {
            let alias = source_alias(other)?;
            if table.is_none_or(|t| t == alias) {
                into.push(build::projection(build::star(Some(alias)), None, None)?);
            }
            Ok(())
        }
    }
}

/// Java's `lateralsOf`: the markers' exploded columns, by alias, anywhere in a FROM.
fn laterals_of(src: &SqlSource, into: &mut Scope) -> Result<(), Refusal> {
    match src {
        SqlSource::Join { left, right, .. } => {
            laterals_of(left, into)?;
            laterals_of(right, into)
        }
        SqlSource::Subselect { inner, alias, .. } => {
            if let SqlQuery::Select(m) = inner.as_ref()
                && is_marker_select(m)
            {
                let mut cols: Vec<(String, SqlExpr)> = Vec::new();
                for p in &m.projections {
                    let name = marker_column(p)?;
                    let xs = unnest_list(&p.expr)?.clone();
                    match cols.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = xs,
                        None => cols.push((name, xs)),
                    }
                }
                match into.iter_mut().find(|(a, _)| a == alias) {
                    Some(slot) => slot.1 = cols,
                    None => into.push((alias.clone(), cols)),
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Java's `inlineable`: a select whose rows can be aggregated in place.
fn inlineable(s: &SqlSelect) -> bool {
    !s.distinct
        && s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.limit.is_none()
        && s.offset.is_none()
        && !matches!(s.from, SqlSource::Dual)
}

// ==================================================================
// The renderer (Java's overrides of AnsiSqlRenderer)
// ==================================================================

impl AnsiSqlRenderer for MariaDb {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        if self.mysql { "MySql" } else { "MariaDb" }
    }

    /// The statement's labels and its root's selects in scope while it is written.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        self.root_selects.borrow_mut().push(None);
        let rendered = ansi::render(self, query);
        self.roots.borrow_mut().pop();
        self.root_selects.borrow_mut().pop();
        rendered
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        self.roots.borrow_mut().push(ansi::query_outputs(&query).to_vec());
        self.root_selects.borrow_mut().push(None);
        let rendered = ansi::render_statement(self, query);
        self.roots.borrow_mut().pop();
        self.root_selects.borrow_mut().pop();
        rendered
    }

    /// The engine's H2 text is refused here: MariaDB has no translation of it.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        let _ = statement;
        Err(Refusal::capability("the engine's H2 text reached MariaDB, which has no translation of it"))
    }

    /// A Float's literal spelling is this dialect's own, the value read once.
    fn spells_float_canon(&self) -> bool {
        true
    }

    /// The portable carrier strategies, star exclusions spelled out, the substring start clamp, explosions placed as
    /// markers, QUALIFY as a wrapping subselect, a windowed GROUP_CONCAT as a correlated subquery, a constant key as an
    /// expression -- and on MySQL each window call inside an expression computed in a select below.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        let mut ps: Vec<Box<dyn SqlRewriter>> = vec![
            Box::new(carrier::CarrierStrategies::new(CAPS)),
            Box::new(shared_passes::StarExceptToColumns),
            Box::new(passes::SubstringClamp),
            Box::new(explosion_markers::LateralUnnestAsMarker),
            Box::new(explosion_markers::SelectListUnnestAsLateral::default()),
            Box::new(explosion_markers::StarOverExplosionUnwrapped),
            Box::new(shared_passes::QualifyToSubselect::by_outputs()),
            Box::new(passes_family::WindowAggregatesAsCorrelated::new(&[AggFn::StringAgg])),
            Box::new(passes::ConstantKeysAsExpressions::default()),
        ];
        if self.mysql {
            ps.push(Box::new(passes_family::WindowsComputedBelow::default()));
        }
        ps
    }

    fn huge_int_literal(&self, v: &BigDecimal) -> String {
        if self.mysql {
            // a HUGEINT literal is a DECIMAL, as MariaDB delivers it (a bare one is a BIGINT UNSIGNED on MySQL)
            return format!("CAST({} AS DECIMAL(38,0))", crate::java::big_decimal_plain(v.as_str()));
        }
        ansi::huge_int_literal(v)
    }

    fn star_except_keyword(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(format!(
            "a star exclusion reached {} unexpanded (StarExceptToColumns)",
            self.product()
        )))
    }

    fn as_of_join_clause(&self) -> Result<String, Refusal> {
        Err(Refusal::capability(format!(
            "an as-of join reached {}, which has none (the carrier strategies emulate the witnessed shapes)",
            self.product()
        )))
    }

    fn pivot_source(&self, w: &mut SqlWriter, pivot: &SqlSource, depth: usize) -> Result<(), Refusal> {
        let _ = (w, pivot, depth);
        Err(Refusal::capability(format!("a pivot reached {} unemulated: it has no PIVOT", self.product())))
    }

    /// Every identifier quoted (ANSI_QUOTES): a double-quoted declaration is its own spelling.
    fn ident(&self, name: &str) -> String {
        if name.len() > 1
            && name.starts_with('"')
            && name.ends_with('"')
            && !name[1..name.len() - 1].replace("\"\"", "").contains('"')
        {
            return name.to_owned();
        }
        self.delimited(name)
    }

    /// The root's selects found when the root is written; a UNION branch with an ORDER BY or LIMIT parenthesized.
    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        if self.pending_root() {
            let selects = selects_of(q);
            let mut root_selects = self.root_selects.borrow_mut();
            root_selects.pop();
            root_selects.push(Some(selects));
        }
        let SqlQuery::Union(u) = q else {
            return ansi::query(self, w, q, depth);
        };
        let op = if u.all { "UNION ALL" } else { "UNION" };
        for (i, b) in u.branches.iter().enumerate() {
            if i > 0 {
                self.nl(w, depth);
                w.append(op);
                self.nl(w, depth);
            }
            let wrap = match b {
                SqlQuery::Select(s) => !s.order_by.is_empty() || s.limit.is_some() || s.offset.is_some(),
                _ => true,
            };
            if wrap {
                w.append("(");
            }
            self.query(w, b, depth)?;
            if wrap {
                w.append(")");
            }
        }
        Ok(())
    }

    /// An ordered derived table takes a LIMIT (MariaDB and MySQL drop a derived table's ORDER BY without one).
    fn subselect_source(&self, w: &mut SqlWriter, inner: &SqlQuery, alias: &str, depth: usize) -> Result<(), Refusal> {
        if let SqlQuery::Select(s) = inner
            && !s.order_by.is_empty()
            && s.limit.is_none()
        {
            let limited = build::query(build::with_limit(s.as_ref().clone(), Some(i64::MAX))?);
            return ansi::subselect_source(self, w, &limited, alias, depth);
        }
        ansi::subselect_source(self, w, inner, alias, depth)
    }

    /// A marker select is its reads over the marker's positions; a select's MODEs counted, its markers in scope, its
    /// stars over them spelled, a root select's values delivered as its labels say; an OFFSET takes a LIMIT.
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if is_marker_select(s) {
            let alias = format!("__u{depth}");
            let aligned = s.outputs.len() == s.projections.len();
            let mut ps = Vec::with_capacity(s.projections.len());
            for (i, p) in s.projections.iter().enumerate() {
                let slot = if aligned { Some(&s.outputs[i].r#type) } else { p.out.as_ref().map(|o| &o.r#type) };
                let read = exploded_read(&alias, p, slot)?;
                ps.push(build::projection(read, Some(marker_column(p)?), p.out.clone())?);
            }
            let position = build::sort_key(build::derived(Some(&alias), "key"), true, None, None);
            let over = build::select(SqlSelect {
                projections: ps,
                distinct: false,
                from: build::subselect(build::query(s.clone()), &alias, None),
                r#where: None,
                group_by: Vec::new(),
                having: None,
                qualify: None,
                order_by: vec![position],
                limit: None,
                offset: None,
                outputs: Vec::new(),
            })?;
            // a root marker's reads are a root select (Java's `top.put(over, TRUE)`)
            let rooted = self.is_root(s);
            if rooted {
                self.mark_root(&over);
            }
            let written = self.select(w, &over, depth);
            if rooted {
                self.unmark_root(&over);
            }
            return written;
        }
        let rooted = self.is_root(s);
        let mut counts = Vec::new();
        let counted = self.modes_counted(s, &mut counts)?;
        let s = counted.unwrap_or_else(|| s.clone());
        let mut scope = Vec::new();
        laterals_of(&s.from, &mut scope)?;
        // a select whose stars are spelled is made anew: no root select, so its values are delivered as they are
        let (spelled, as_text) =
            if scope.is_empty() { self.delivered(s, rooted)? } else { (stars_spelled(s)?, Vec::new()) };
        self.laterals.borrow_mut().push(scope);
        self.mode_counts.borrow_mut().push(counts);
        let written = (|| {
            let s = if spelled.offset.is_some() && spelled.limit.is_none() {
                build::with_limit(spelled, Some(i64::MAX))?
            } else {
                spelled
            };
            let addresses: Vec<usize> = as_text.iter().map(|&i| address_of(&s.projections[i].expr)).collect();
            self.as_text.borrow_mut().extend(addresses.iter().copied());
            let written = ansi::select(self, w, &s, depth);
            // a value not written was never read again (Java's entry left in the set, which no other node matches)
            self.as_text.borrow_mut().retain(|a| !addresses.contains(a));
            written
        })();
        self.mode_counts.borrow_mut().pop();
        self.laterals.borrow_mut().pop();
        written
    }

    /// A marker's positions (a lateral one joined correlated); any other lateral derived table its rows as JSON, read
    /// back by `JSON_TABLE` (MySQL has LATERAL).
    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        if let SqlSource::Join {
            left, right, kind: kind @ (JoinKind::CrossLateral | JoinKind::LeftLateral), on, ..
        } = src
            && let SqlSource::Subselect { inner, alias, .. } = right.as_ref()
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            if let Some(on) = on
                && !matches!(on.as_ref(), SqlExpr::BoolLit { value: true, .. })
            {
                return Err(Refusal::capability(format!(
                    "a lateral explosion with a join condition reached {}",
                    self.product()
                )));
            }
            self.source(w, left, depth)?;
            self.nl(w, depth);
            let is_left = *kind == JoinKind::LeftLateral;
            w.append(if is_left { "LEFT JOIN " } else { "CROSS JOIN " });
            let positions = self.positions(m, self.alias_ident(alias))?;
            w.piece(&*positions)?;
            if is_left {
                w.append(" ON TRUE");
            }
            return Ok(());
        }
        if let SqlSource::Subselect { inner, alias, .. } = src
            && let SqlQuery::Select(m) = inner.as_ref()
            && is_marker_select(m)
        {
            let positions = self.positions(m, self.alias_ident(alias))?;
            w.piece(&*positions)?;
            return Ok(());
        }
        if let SqlSource::Join {
            left, right, kind: kind @ (JoinKind::CrossLateral | JoinKind::LeftLateral), on, ..
        } = src
        {
            if self.correlates_derived() {
                // a database with LATERAL (MySQL)
                return ansi::source(self, w, src, depth);
            }
            let SqlSource::Subselect { inner, alias, .. } = right.as_ref() else {
                return Err(Refusal::capability(format!(
                    "a lateral join to a {} reached {}, which has no LATERAL",
                    source_class(right),
                    self.product()
                )));
            };
            let is_left = *kind == JoinKind::LeftLateral;
            let always = on.as_deref().is_none_or(|on| matches!(on, SqlExpr::BoolLit { value: true, .. }));
            self.source(w, left, depth)?;
            self.nl(w, depth);
            w.append(if is_left {
                "LEFT JOIN "
            } else if always {
                "CROSS JOIN "
            } else {
                "JOIN "
            });
            let rows = self.lateral_rows(inner, depth)?;
            w.piece(&*rows)?.append(" AS ").append(&self.alias_ident(alias));
            if is_left || !always {
                w.append(" ON ");
                if always {
                    w.append("TRUE");
                } else {
                    w.expr(self, on.as_deref().ok_or_else(|| Refusal::null_pointer("on"))?, 0)?;
                }
            }
            return Ok(());
        }
        ansi::source(self, w, src, depth)
    }

    /// No VALUES alias list: the rows a union of one-row selects naming their columns; no rows one row of NULLs,
    /// filtered.
    fn values_source(
        &self,
        w: &mut SqlWriter,
        rows: &[Vec<SqlExpr>],
        columns: &[String],
        alias: &str,
        outputs: &[OutputCol],
    ) -> Result<(), Refusal> {
        let _ = outputs;
        w.append("(");
        if rows.is_empty() {
            w.append("SELECT ");
            for (i, c) in columns.iter().enumerate() {
                w.append(if i > 0 { ", " } else { "" }).append("NULL AS ").append(&self.alias_ident(c));
            }
            w.append(" FROM DUAL WHERE FALSE) AS ").append(&self.alias_ident(alias));
            return Ok(());
        }
        for (r, row) in rows.iter().enumerate() {
            w.append(if r > 0 { " UNION ALL SELECT " } else { "SELECT " });
            for (i, cell) in row.iter().enumerate() {
                w.append(if i > 0 { ", " } else { "" }).expr(self, cell, 0)?;
                if r == 0 {
                    let name = columns.get(i).ok_or_else(|| Refusal {
                        class: Refusal::INDEX_OUT_OF_BOUNDS,
                        message: format!("Index {i} out of bounds for length {}", columns.len()),
                    })?;
                    w.append(" AS ").append(&self.alias_ident(name));
                }
            }
        }
        w.append(") AS ").append(&self.alias_ident(alias));
        Ok(())
    }

    // ---- lexical: identifiers, literals, types

    fn string_lit(&self, value: &str) -> Result<String, Refusal> {
        Ok(self.string_literal(value))
    }

    fn date_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("DATE '{iso}'"))
    }

    /// A date-time literal at microsecond precision.
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        Ok(format!("CAST('{}' AS DATETIME(6))", iso.replace('T', " ")))
    }

    /// A Float literal is a DOUBLE only with an exponent; no infinity, no NaN.
    fn float_literal(&self, v: f64) -> Result<String, Refusal> {
        let s = lite_javacompat::double_to_string(v);
        if v.is_nan() || v.is_infinite() {
            return Err(Refusal::capability(format!(
                "a {s} literal reached {}, whose DOUBLE has no infinity or NaN",
                self.product()
            )));
        }
        Ok(if s.contains('E') { s } else { format!("{s}E0") })
    }

    fn array_type_name(&self, element: &str) -> Result<String, Refusal> {
        let _ = element;
        Ok("LONGTEXT".to_owned())
    }

    fn map_type_name(&self, key: &str, value: &str) -> String {
        let _ = (key, value);
        "LONGTEXT".to_owned()
    }

    /// No STRUCT type here: a struct cast is refused.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let _ = fields;
        Err(Refusal::capability("a STRUCT type reached a dialect without struct support"))
    }

    /// Every scalar's name (MySql's, by its exhaustive switch, MariaDb's).
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean | Scalar::Integer | Scalar::Bigint => "SIGNED",
            Scalar::Hugeint => "DECIMAL(38,0)",
            Scalar::Double => "DOUBLE",
            Scalar::Varchar | Scalar::Json | Scalar::TemporalText => "CHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp | Scalar::Timestamptz => "DATETIME(6)",
            Scalar::Time => "TIME(6)",
            Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    /// The base's strftime codes (MySql's, by delegation, MariaDb's).
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(ansi::strftime_text(parts))
    }

    /// Alias-less projections label EXPLICITLY from the declared output.
    fn implicit_label(&self, p: &Projection) -> Result<Option<String>, Refusal> {
        match &p.out {
            Some(out) => Ok(Some(self.alias_ident(&out.name))),
            None => ansi::implicit_label(self, p),
        }
    }

    /// NULL sorts first ascending here and there is no NULLS FIRST/LAST: the placement a key of its own before the
    /// value, in the value's direction.
    fn sort_key(&self, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
        let nulls_first = match k.null_order {
            Some(order) => order == NullOrder::NullsFirst,
            None => !k.ascending,
        };
        let dir = if k.ascending { "" } else { " DESC" };
        if !matches!(k.expr, SqlExpr::IntLit { .. } | SqlExpr::StringLit { .. }) {
            w.append("(").expr(self, &k.expr, 4)?;
            w.append(if nulls_first == k.ascending { " IS NOT NULL)" } else { " IS NULL)" });
            w.append(dir).append(", ");
        }
        w.expr(self, &k.expr, 0)?.append(dir);
        Ok(())
    }

    fn agg_order_null_placement(&self, k: &SortKey) -> String {
        let _ = k;
        String::new()
    }

    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        let address = address_of(e);
        let as_text = {
            let mut set = self.as_text.borrow_mut();
            match set.iter().position(|a| *a == address) {
                Some(at) => {
                    set.remove(at);
                    true
                }
                None => false,
            }
        };
        if as_text {
            w.append("CAST(");
            self.expr(w, e, 0)?;
            w.append(" AS CHAR)");
            return Ok(());
        }
        match e {
            SqlExpr::ArrayLit { elements, .. } => self.array_lit(w, elements),
            SqlExpr::BoolLit { value, .. } => {
                w.append(if *value { "TRUE" } else { "FALSE" });
                Ok(())
            }
            // DuckDB's list(v ORDER BY k): ascending, NULLs last
            SqlExpr::OrderedListAgg { value, order_by, .. } => {
                let key = build::sort_key(order_by.as_ref().clone(), true, Some(NullOrder::NullsLast), None);
                let order = pc(move |o| self.sort_key(o, &key));
                let agg = self.array_agg(false, self.json(value), Some(order), false);
                w.piece(&*agg)?;
                Ok(())
            }
            SqlExpr::Membership { collection, .. } if matches!(collection.as_ref(), SqlExpr::PlanParam { .. }) => {
                let SqlExpr::PlanParam { name, .. } = collection.as_ref() else { unreachable!("a plan parameter") };
                Err(self.list_parameter(name))
            }
            SqlExpr::Column(c) => match self.exploded_column(c) {
                Some((xs, position)) => self.element(w, &xs, &position),
                None => ansi::expr(self, w, e, parent_prec),
            },
            other => ansi::expr(self, w, other, parent_prec),
        }
    }

    fn array_lit(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_ARRAY(").join(elements, ", ", |w, x| {
            let json = self.json(x);
            w.piece(&*json)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    fn struct_lit(&self, w: &mut SqlWriter, fields: &[StructLitField]) -> Result<(), Refusal> {
        w.append("JSON_OBJECT(").join(fields, ", ", |w, f| {
            w.append(&self.string_literal(&f.name)).append(", ");
            let json = self.json(&f.value);
            w.piece(&*json)?;
            Ok(())
        })?;
        w.append(")");
        Ok(())
    }

    /// A field of a JSON object, read as its type.
    fn struct_get(&self, w: &mut SqlWriter, source: &SqlExpr, field: &str, r#type: &TypeFact) -> Result<(), Refusal> {
        let mut t = fact_type(r#type);
        if t.is_none()
            && let Some(SqlType::Struct { fields }) = type_of(source)
        {
            for f in fields {
                if f.name == field {
                    t = Some(&f.r#type);
                }
            }
        }
        let path = text(self.member_path(field));
        let at = self.value_at(self.ex(source, 0), path, t);
        w.piece(&*at)?;
        Ok(())
    }

    fn struct_insert(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.struct_insert_arm(w, args)
    }

    fn list_exists(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "max", "FALSE", SqlFn::ListExists)
    }

    fn list_for_all(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.list_predicate(w, args, "min", "TRUE", SqlFn::ListForAll)
    }

    fn all_distinct(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.all_distinct_arm(w, args)
    }

    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        self.membership_arm(w, needle, collection)
    }

    fn reduce_collection(
        &self,
        w: &mut SqlWriter,
        reducer: AggFn,
        collection: &SqlExpr,
        extras: &[SqlExpr],
    ) -> Result<(), Refusal> {
        let rows = self.over_rows(collection, reducer, extras)?;
        w.piece(&*rows)?;
        Ok(())
    }

    fn unnest_projection(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let _ = (w, args);
        Err(Refusal::capability(format!(
            "an unnest in a select list reached {}, which has no set-returning projection (an explode is placed in \
             FROM as a JSON_TABLE)",
            self.product()
        )))
    }

    fn fold_call(&self, w: &mut SqlWriter, fold: &SqlExpr) -> Result<(), Refusal> {
        self.fold_arm(w, fold)
    }

    fn split_part_call(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.split_part_arm(w, args)
    }

    fn variant_construct(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() != 1 {
            return Err(Refusal::capability(format!("toVariant of {} arguments reached {}", a.len(), self.product())));
        }
        let v = self.variant_of(&a[0]);
        w.piece(&*v)?;
        Ok(())
    }

    fn variant_get(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        self.variant_get_arm(w, args)
    }

    fn variant_elements(&self, w: &mut SqlWriter, args: &[SqlExpr]) -> Result<(), Refusal> {
        let x = arg(args, 0)?;
        w.append("(CASE WHEN JSON_TYPE(").expr(self, x, 0)?.append(") = 'ARRAY' THEN ").expr(self, x, 0)?;
        w.append(" END)");
        Ok(())
    }

    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        let _ = conform;
        self.cast_arm(w, value, target)
    }

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], t: &TypeFact, parent_prec: i32) -> Result<(), Refusal> {
        self.call_arm(w, f, a, t, parent_prec)
    }

    /// A CASE whose branch is error() raises in the CASE's own type.
    fn case_expr(
        &self,
        w: &mut SqlWriter,
        whens: &[When],
        otherwise: Option<&SqlExpr>,
        r#type: &TypeFact,
    ) -> Result<(), Refusal> {
        let slot = fact_type(r#type);
        w.append("CASE");
        for wh in whens {
            w.append(" WHEN ").expr(self, &wh.condition, 0)?.append(" THEN ");
            let branch = self.branch(&wh.then, slot)?;
            w.piece(&*branch)?;
        }
        if let Some(o) = otherwise {
            w.append(" ELSE ");
            let branch = self.branch(o, slot)?;
            w.piece(&*branch)?;
        }
        w.append(" END");
        Ok(())
    }

    fn hash_signed(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.hash_signed_arm(w, a)
    }

    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.round_half_even_arm(w, a)
    }

    fn bit_op(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr]) -> Result<(), Refusal> {
        self.bit_op_arm(w, f, a)
    }

    /// Every aggregate (MySql's, by its exhaustive switch, MariaDb's).
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        self.reducer_arm(w, r)
    }

    fn window_call(
        &self,
        w: &mut SqlWriter,
        r#fn: &SqlAgg,
        partition_by: &[SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        if self.mysql
            && let SqlAgg::Reducer(r) = r#fn
            && matches!(r.r#fn, AggFn::Median | AggFn::QuantileCont | AggFn::QuantileDisc)
            && (!order_by.is_empty() || frame.is_some())
        {
            return Err(Refusal::capability(format!(
                "a windowed {} over an ordered or framed window reached MySQL, which has no PERCENTILE_CONT",
                r.r#fn
            )));
        }
        self.window_call_arm(w, r#fn, partition_by, order_by, frame)
    }

    fn json_object(&self, w: &mut SqlWriter, kv: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("JSON_OBJECT(");
        for (i, x) in kv.iter().enumerate() {
            w.append(if i > 0 { ", " } else { "" });
            if i % 2 == 0 {
                w.expr(self, x, 0)?;
            } else {
                let json = self.json(x);
                w.piece(&*json)?;
            }
        }
        w.append(")");
        Ok(())
    }

    fn json_array(&self, w: &mut SqlWriter, elements: &[SqlExpr]) -> Result<(), Refusal> {
        self.array_lit(w, elements)
    }

    /// A JSON array of the values, NULLs kept; over no rows the empty array.
    fn json_array_agg(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        order_keys: &[JsonArrayAggKey],
    ) -> Result<(), Refusal> {
        let order = if order_keys.is_empty() {
            None
        } else {
            Some(pc(move |v| {
                v.join(order_keys, ", ", |w, k| {
                    let key = aggregates::element_sort_of(self.ex(&k.expr, 0), !k.desc);
                    w.piece(&*key)?;
                    Ok(())
                })?;
                Ok(())
            }))
        };
        let agg = self.array_agg(false, self.json(value), order, false);
        w.append("coalesce(").piece(&*agg)?.append(", JSON_ARRAY())");
        Ok(())
    }

    fn hole_type(&self, kind: ValueKind) -> Result<TypeSpelling, Refusal> {
        aggregates::hole_type(kind)
    }

    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        aggregates::ddl_type(t)
    }

    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(aggregates::json_kind_name(kind).to_owned())
    }

    /// Refused, as TYPEOF is here (its type names are DuckDB's).
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        let _ = scalar;
        Err(Refusal::capability("typeof reached MariaDB: its type names are DuckDB's"))
    }
}

impl MariaDb {
    /// Java's `listParameter`: a list plan parameter, which MariaDB binds no array of.
    fn list_parameter(&self, name: &str) -> Refusal {
        Refusal::capability(format!("list plan parameter '{name}' reached {}, which binds no array", self.product()))
    }

    /// Java's `wall`.
    fn wall(&self, f: SqlFn, why: &str) -> Refusal {
        Refusal::capability(format!("{f} reached {}: {why}", self.product()))
    }

    /// Java's `literal`: a string literal's value, else refused naming `what`.
    fn literal<'e>(&self, e: &'e SqlExpr, what: &str) -> Result<&'e str, Refusal> {
        match e {
            SqlExpr::StringLit { value, .. } => Ok(value),
            _ => Err(Refusal::capability(format!("{what} that is not a literal reached {}", self.product()))),
        }
    }

    /// The root mark of a select made in a root select's stead (Java's `top.put(over, TRUE)`), until it is written.
    fn mark_root(&self, s: &SqlSelect) {
        if let Some(Some(selects)) = self.root_selects.borrow_mut().last_mut() {
            selects.push(address_of_select(s));
        }
    }

    fn unmark_root(&self, s: &SqlSelect) {
        let address = address_of_select(s);
        if let Some(Some(selects)) = self.root_selects.borrow_mut().last_mut()
            && let Some(at) = selects.iter().rposition(|a| *a == address)
        {
            selects.remove(at);
        }
    }
}

#[cfg(test)]
mod tests;

// ==================================================================
// SqlDialect
// ==================================================================

impl crate::dialect::SqlDialect for MariaDb {
    fn dialect_name(&self) -> &'static str {
        AnsiSqlRenderer::simple_name(self)
    }

    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        AnsiSqlRenderer::render(self, query)
    }

    fn render_inline(&self, query: SqlQuery) -> Result<String, Refusal> {
        ansi::render_inline(self, query)
    }

    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_statement(self, query)
    }

    fn render_ddl(&self, ddl: &SqlDdl) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_ddl(self, ddl)
    }

    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        AnsiSqlRenderer::render_dml(self, dml)
    }

    fn render_dml_statement(&self, dml: &SqlDml) -> Result<RenderedStatement, Refusal> {
        AnsiSqlRenderer::render_dml_statement(self, dml)
    }

    fn raw(&self, statement: &str, written_for: Option<&str>) -> Result<String, Refusal> {
        ansi::raw(self, statement, written_for)
    }

    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        AnsiSqlRenderer::from_engine_text(self, statement)
    }

    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::stored_name(self, name, declared_quoted)
    }

    fn physical_name(&self, name: &str) -> String {
        AnsiSqlRenderer::physical_name(self, name)
    }

    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        AnsiSqlRenderer::insert_column_name(self, name, declared_quoted)
    }

    fn reads_stored(&self, t: &ColumnType) -> bool {
        <MariaDb as AnsiSqlRenderer>::reads_stored(t)
    }

    fn table_name_spelled(&self, name: &str, quoting: NameQuoting) -> String {
        AnsiSqlRenderer::table_name(self, name, quoting)
    }

    /// The session the spellings assume: ANSI_QUOTES and the rest of the sql_mode, UTC, utf8mb4 binary, the
    /// recursion and GROUP_CONCAT lengths, the division precision (MariaDB: no condition pushed into a derived table;
    /// MySQL: no unsigned subtraction, its own recursion depth).
    fn session_setup(&self) -> Vec<String> {
        let lines: &[&str] = if self.mysql {
            &[
                "SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,\
                 NO_ENGINE_SUBSTITUTION,NO_UNSIGNED_SUBTRACTION'",
                "SET SESSION time_zone = '+00:00'",
                "SET NAMES utf8mb4 COLLATE utf8mb4_bin",
                "SET SESSION group_concat_max_len = 1073741824",
                "SET SESSION cte_max_recursion_depth = 100000000",
                "SET SESSION div_precision_increment = 30",
            ]
        } else {
            &[
                "SET SESSION optimizer_switch = 'condition_pushdown_for_derived=off'",
                "SET SESSION sql_mode = 'ANSI_QUOTES,PIPES_AS_CONCAT,NO_BACKSLASH_ESCAPES,STRICT_ALL_TABLES,\
                 NO_ENGINE_SUBSTITUTION'",
                "SET SESSION time_zone = '+00:00'",
                "SET NAMES utf8mb4 COLLATE utf8mb4_bin",
                "SET SESSION group_concat_max_len = 1073741824",
                "SET SESSION max_recursive_iterations = 100000000",
                "SET SESSION div_precision_increment = 30",
            ]
        };
        lines.iter().map(|s| (*s).to_owned()).collect()
    }

    /// InnoDB's REPEATABLE READ reads one snapshot.
    fn keys_snapshot(&self) -> Result<KeysSnapshot, Refusal> {
        Ok(KeysSnapshot::RepeatableRead)
    }

    /// 4,096 columns (the documented column limit).
    fn result_column_limit(&self) -> Result<Option<i32>, Refusal> {
        Ok(Some(4096))
    }

    /// No native pivot: a static pivot is emulated, so a dynamic one is discovered first.
    fn needs_static_pivot(&self) -> bool {
        true
    }

    fn wire_type_name(&self, driver_type_name: Option<&str>) -> String {
        aggregates::wire_type_name(driver_type_name)
    }

    fn delivers_own(&self, label: &SqlType, wire: &str) -> bool {
        aggregates::delivers_own(label, wire)
    }
}
