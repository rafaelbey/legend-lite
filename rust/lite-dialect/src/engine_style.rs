// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's engine-text dialects (`core/src/main/java/com/legend/sql/dialect/EngineStyleH2.java`, `EngineStyleDB2.java`,
//! `EngineStyleComposite.java`): legend-engine's OWN SQL text -- the `toSQLString` and plan goldens, never executed --
//! one line, lowercase keywords, each select's leftmost source aliased `"root"` and every other source
//! `<lowercase-table>_<n>` by the engine's reAliasQuery, physical names bare, ON clauses parenthesized, plan
//! parameters as the engine's FreeMarker templates.
//!
//! The three are this struct, by [`Profile`]: Java's `EngineStyleDB2 extends EngineStyleH2` and
//! `EngineStyleComposite extends EngineStyleDB2`, and each override of theirs is a branch on the profile in the method
//! Java overrides -- DB2's `expr` (the null-safe OR expansions), `whereSql`, `nullSafeEq`, `holderArgs`, `groupKey`,
//! `groupBySeparator`, `infixPad`, `variantAwareCast` and `call` ([`EngineStyle::db2_call`]), Composite's `call`
//! ([`EngineStyle::composite_call`]); a `super.call` is [`EngineStyle::h2_call`]. DB2's and Composite's
//! `scalarTypeName`, `formatText` and `reducer` delegate every constant to their parent's, so they are H2's here.
//! Every other answer is `EngineStyleH2.java`'s, and the refusals that name the dialect name it by its own simple name
//! (`AnsiSqlRenderer`'s and `SqlDialect`'s defaults: `getClass().getSimpleName()`).
//!
//! A render plans the engine's aliases into the dialect object (Java's `renames`, `subselects`, `rootConsumed`,
//! `placeholders`, cleared at each plan) and tracks the frames it is inside (`frameDepth`, `anonDistinctDepth`);
//! `renderedAlias` reads the last plan. A fresh dialect per render is exact (the oracles' answers are fresh
//! dialects'); `EngineTextDialect::plan_aliases` rebuilds a render's plan for the seam, which holds no dialect between
//! calls.

use crate::ansi::{self, flatten_concat, part};
use crate::carrier::CarrierStrategies;
use crate::decode_shapes;
use crate::dialect::{EngineTextDialect, SqlDialect};
use crate::h2::{CAPS, ENGINE_RESERVED};
use crate::h2_passes::LateralExplodeToUnion;
use crate::java::{big_decimal_plain, expr_text, simple_name};
use crate::java_time;
use crate::refusal::{Refusal, arg, tail};
use crate::renderer::{AnsiSqlRenderer, AnsiState, Lexicon, PlanParam};
use crate::rewriter::SqlRewriter;
use crate::statement::{RenderedStatement, utf16_len};
use crate::writer::SqlWriter;
use lite_sql::{
    AggFn, ColumnKind, ColumnType, DateFmt, DateFmtPart, DatePart, Frame, JoinKind, JsonKind, ParamKind, Projection,
    Reducer, Scalar, SortKey, SqlAgg, SqlDdl, SqlDml, SqlExpr, SqlFn, SqlQuery, SqlSelect, SqlSource, SqlType,
    StructField,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::LazyLock;

/// Which of Java's three classes the dialect is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// `EngineStyleH2`: the engine's H2 text.
    H2,
    /// `EngineStyleDB2`: the engine's DB2 text, H2's but for the forms DB2's goldens pin.
    Db2,
    /// `EngineStyleComposite`: DB2's, but `char_length` and the full `substring` keyword.
    Composite,
}

impl Profile {
    /// Java's `getClass().getSimpleName()`.
    pub fn simple_name(self) -> &'static str {
        match self {
            Profile::H2 => "EngineStyleH2",
            Profile::Db2 => "EngineStyleDB2",
            Profile::Composite => "EngineStyleComposite",
        }
    }

    /// The profile a simple name names.
    pub fn of(simple_name: &str) -> Option<Profile> {
        [Profile::H2, Profile::Db2, Profile::Composite].into_iter().find(|p| p.simple_name() == simple_name)
    }

    /// `instanceof EngineStyleDB2`: DB2 and its subclass Composite.
    fn db2(self) -> bool {
        matches!(self, Profile::Db2 | Profile::Composite)
    }
}

/// Java's `EngineStyleH2.LEXICON`: minimal quoting, no reserved word.
static LEXICON: LazyLock<Lexicon> = LazyLock::new(|| Lexicon::new('"', &[]));

/// Java's `SqlSelect.SYNTH_MAP_COL`: a scalar-map column's alias prefix, which the engine spells unaliased.
const SYNTH_MAP_COL: &str = "u_map__";

/// Java's `SqlSource.Subselect.EXISTS_KEYS_FRAME`: the join-distinct exists key subselect's frame marker.
const EXISTS_KEYS_FRAME: &str = "existsKeys";

/// Java's `SqlExpr.TDS_NULL_CELL`.
const TDS_NULL_CELL: &str = "TDSNull";

/// A union frame's name.
const UNION_ALIAS: &str = "unionAlias";

/// What `quotedFrameRead` reads of a planned subselect (Java keeps the subselect itself; only this of it is read).
#[derive(Debug)]
enum FrameRead {
    /// A union frame: the names of its query's outputs (a read of one spells the quoted output alias).
    Union(Vec<String>),
    /// A plain frame (no frame name, a select that is not DISTINCT): its projections' aliases.
    Aliases(Vec<String>),
    /// Any other frame: its reads stay physical.
    Physical,
}

/// One render's alias plan (Java's `renames`, `subselects`, `rootConsumed` and `placeholders`).
#[derive(Debug, Default)]
struct AliasPlan {
    renames: HashMap<String, String>,
    subselects: HashMap<String, FrameRead>,
    root_consumed: HashSet<String>,
    placeholders: HashSet<String>,
}

/// An engine-text dialect (Java's `new EngineStyleH2(quoteIdentifiers, timeZone)`, `new EngineStyleDB2(...)`, `new
/// EngineStyleComposite()`).
#[derive(Debug)]
pub struct EngineStyle {
    ansi: AnsiState,
    profile: Profile,
    /// The engine connection's `quoteIdentifiers`: every physical identifier double-quoted.
    quote_identifiers: bool,
    /// The connection's time zone: a non-default one wraps DATETIME placeholders in `GMTtoTZ` and shifts date-time
    /// constants (`GMT` shifts none).
    time_zone: Option<String>,
    plan: RefCell<AliasPlan>,
    /// Nesting of named (view) frames being written: their projections are aliased unquoted.
    frame_depth: Cell<i32>,
    /// Nesting of anonymous exists-key frames being written: their DISTINCT keys are bare.
    anon_distinct_depth: Cell<i32>,
}

impl EngineStyle {
    /// Java's `new EngineStyleH2(quoteIdentifiers, timeZone)` and its subclasses' (`new EngineStyleH2()` is
    /// `(false, null)`; Composite has only that).
    pub fn new(profile: Profile, quote_identifiers: bool, time_zone: Option<String>) -> EngineStyle {
        EngineStyle {
            ansi: AnsiState::default(),
            profile,
            quote_identifiers,
            time_zone,
            plan: RefCell::default(),
            frame_depth: Cell::new(0),
            anon_distinct_depth: Cell::new(0),
        }
    }

    /// Java's `new EngineStyleH2()`.
    pub fn h2() -> EngineStyle {
        EngineStyle::new(Profile::H2, false, None)
    }

    /// Java's `new EngineStyleDB2()`.
    pub fn db2() -> EngineStyle {
        EngineStyle::new(Profile::Db2, false, None)
    }

    /// Java's `new EngineStyleComposite()`.
    pub fn composite() -> EngineStyle {
        EngineStyle::new(Profile::Composite, false, None)
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    // ==================================================================
    // The plan: passes, the TDS join's wrap, the alias plan
    // ==================================================================

    /// The query as this text writes it (Java's `planned`): every render pass applied, a TDS join's top wrapped, and
    /// its alias plan made (`realias`: the engine's reAliasQuery) or none (it keeps the aliases it carries).
    fn planned(&self, query: SqlQuery, realias: bool) -> Result<SqlQuery, Refusal> {
        let query = wrap_tds_join_top(ansi::passed(self, query)?)?;
        let mut plan = self.plan.borrow_mut();
        *plan = AliasPlan::default();
        if realias {
            plan.plan_query(&query, &mut HashMap::new(), true);
        }
        Ok(query)
    }

    /// `renames.getOrDefault(alias, alias)`.
    fn rename(&self, alias: &str) -> String {
        self.plan.borrow().renames.get(alias).cloned().unwrap_or_else(|| alias.to_owned())
    }

    /// Whether a column reads an aliased projection of a plain frame, or a union frame's output: a quoted read.
    fn quoted_frame_read(&self, table: &str, name: &str) -> bool {
        match self.plan.borrow().subselects.get(table) {
            None | Some(FrameRead::Physical) => false,
            Some(FrameRead::Union(outputs)) => outputs.iter().any(|o| o == name),
            Some(FrameRead::Aliases(aliases)) => aliases.iter().any(|a| a == name),
        }
    }

    fn is_placeholder(&self, table: &str) -> bool {
        self.plan.borrow().placeholders.contains(table)
    }

    // ==================================================================
    // Select assembly
    // ==================================================================

    /// A select's head after `select`: its row cap as H2's `top` (a slice is OFFSET .. FETCH NEXT), its `distinct`, its
    /// projections -- an anonymous DISTINCT-key frame's self-aliased columns bare.
    fn select_head(&self, w: &mut SqlWriter, s: &SqlSelect) -> Result<(), Refusal> {
        if let Some(limit) = s.limit
            && s.offset.is_none()
        {
            w.append("top ").append_long(limit).append(" ");
        }
        if s.distinct {
            w.append("distinct ");
        }
        let bare_keys = s.distinct && self.anon_distinct_depth.get() > 0;
        if s.projections.is_empty() {
            w.append("*");
            return Ok(());
        }
        w.join(&s.projections, ", ", |w, p| {
            if bare_keys
                && let SqlExpr::Column(pc) = &p.expr
                && p.alias.as_deref() == Some(pc.name.as_str())
            {
                return self.write_projection(w, &p.expr, None);
            }
            self.projection(w, p)
        })?;
        Ok(())
    }

    /// `projection(new Projection(expr, alias, out))`: a projected date constant a plain string, a scalar-map column
    /// unaliased, a view frame's projections aliased unquoted, any other alias quoted once.
    fn write_projection(&self, w: &mut SqlWriter, expr: &SqlExpr, alias: Option<&str>) -> Result<(), Refusal> {
        if let SqlExpr::DateLit { iso, .. } = expr {
            w.append("'").append(iso).append("'");
        } else {
            w.expr(self, expr, 0)?;
        }
        if alias.is_some_and(|a| a.starts_with(SYNTH_MAP_COL)) {
            return Ok(());
        }
        if self.frame_depth.get() > 0
            && let Some(name) = output_name(expr, alias)
        {
            w.append(" as ").append(&name.replace('"', ""));
            return Ok(());
        }
        let Some(a) = alias else {
            return Ok(());
        };
        // a pre-quoted alias keeps its one quote level
        if a.len() > 1 && a.starts_with('"') && a.ends_with('"') {
            w.append(" as ").append(a);
        } else {
            w.append(" as \"").append(a).append("\"");
        }
        Ok(())
    }

    /// The WHERE clause: DB2 wraps a top-level conjunction one of whose conjuncts is OR-rooted.
    fn where_sql(&self, w: &mut SqlWriter, wh: &SqlExpr) -> Result<(), Refusal> {
        if self.profile.db2()
            && let SqlExpr::Call { r#fn: SqlFn::And, args, .. } = wh
            && args.iter().any(or_rooted)
        {
            w.append("(").expr(self, wh, 0)?.append(")");
            return Ok(());
        }
        w.expr(self, wh, 0)?;
        Ok(())
    }

    /// A group-by key: H2 spells a key projected under a renaming alias by its quoted output name; DB2 spells every key
    /// as its expression.
    fn group_key(&self, w: &mut SqlWriter, s: &SqlSelect, e: &SqlExpr) -> Result<(), Refusal> {
        if self.profile.db2() {
            w.expr(self, e, 0)?;
            return Ok(());
        }
        if let SqlExpr::Column(c) = e
            && c.table.is_none()
        {
            // a self-aliased key spells its physical expression; a renaming alias the quoted output name
            for p in &s.projections {
                if output_name(&p.expr, p.alias.as_deref()).as_deref() == Some(c.name.as_str())
                    && let SqlExpr::Column(pc) = &p.expr
                    && pc.name == c.name
                {
                    w.expr(self, &p.expr, 0)?;
                    return Ok(());
                }
            }
            w.append("\"").append(&c.name).append("\"");
            return Ok(());
        }
        for p in &s.projections {
            if let Some(name) = output_name(&p.expr, p.alias.as_deref())
                && *e == p.expr
            {
                // a union frame's reads always spell the quoted output name
                let union_read = matches!(&p.expr, SqlExpr::Column(uc)
                    if matches!(&s.from, SqlSource::Subselect { alias, frame_name: Some(f), .. }
                        if f == UNION_ALIAS && uc.table.as_deref() == Some(alias.as_str())));
                if !union_read
                    && let SqlExpr::Column(pc) = &p.expr
                    && pc.name == name
                {
                    w.expr(self, e, 0)?;
                    return Ok(());
                }
                w.append("\"").append(&name.replace('"', "")).append("\"");
                return Ok(());
            }
        }
        w.expr(self, e, 0)?;
        Ok(())
    }

    /// Group-by keys' separator: H2's `", "`, DB2's bare comma.
    fn group_by_separator(&self) -> &'static str {
        if self.profile.db2() { "," } else { ", " }
    }

    // ==================================================================
    // Expressions: the engine's own forms
    // ==================================================================

    /// `EngineStyleH2.expr`, after DB2's null-safe arms.
    fn h2_expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        // a Float literal: cast(x as float), the engine's current H2 text
        if let SqlExpr::FloatLit { value, .. } = e {
            w.append("cast(").append(&lite_javacompat::double_to_string(value.value())).append(" as float)");
            return Ok(());
        }
        if let SqlExpr::DecimalLit { value, .. } = e {
            w.append("cast(").append(&big_decimal_plain(value.as_str())).append(" as Decimal(32,16))");
            return Ok(());
        }
        if let SqlExpr::Exists { subquery, .. } = e
            && let SqlQuery::Select(xs) = subquery.as_ref()
        {
            return self.correlated_exists(w, xs);
        }
        if let SqlExpr::Call { r#fn, args, .. } = e
            && mixed_operand_arithmetic(*r#fn, args, parent_prec)
        {
            w.append("(");
            ansi::expr(self, w, e, 0)?;
            w.append(")");
            return Ok(());
        }
        if let Some(p) = PlanParam::of(e) {
            return self.plan_param(w, &p);
        }
        // an enum parameter's comparison: one selector template; its negation a lowercase not around it
        if let SqlExpr::Call { r#fn: SqlFn::Not, args, .. } = e
            && args.len() == 1
            && let Some(selector) = self.enum_selector(&args[0])?
        {
            w.append("not ").append(&selector);
            return Ok(());
        }
        if let Some(selector) = self.enum_selector(e)? {
            w.append(&selector);
            return Ok(());
        }
        if self.optional_param_equality(w, e)? {
            return Ok(());
        }
        // a property read through a plan parameter: the dotted placeholder, nested reads walked
        if let SqlExpr::StructGet { .. } = e {
            let mut path = VecDeque::new();
            let mut cur = e;
            while let SqlExpr::StructGet { source, field, .. } = cur {
                path.push_front(field.as_str());
                cur = source;
            }
            if let Some(pp) = PlanParam::of(cur) {
                let fields: Vec<&str> = path.into_iter().collect();
                w.append("'${").append(pp.name).append(".").append(&fields.join(".")).append("}'");
                return Ok(());
            }
            return Err(Refusal {
                class: Refusal::UNSUPPORTED_OPERATION,
                message: "plan: struct extraction has no engine-H2 spelling".to_owned(),
            });
        }
        // a qualified star's frame alias renames like a column read
        if let SqlExpr::Star { table: Some(t), .. } = e {
            w.append("\"").append(&self.rename(t)).append("\".*");
            return Ok(());
        }
        // alias part quoted, physical column bare; a frame's projected aliases quoted
        if let SqlExpr::Column(c) = e {
            if let Some(t) = &c.table
                && self.quoted_frame_read(t, &c.name)
            {
                if self.is_placeholder(t) {
                    w.append("\"").append(&self.rename(t)).append("\".").append(strip_quotes(&c.name));
                } else {
                    w.append("\"").append(&self.rename(t)).append("\".\"").append(&c.name).append("\"");
                }
                return Ok(());
            }
            let column = self.phys_column(c.origin, &c.name);
            match &c.table {
                None => w.append(&column),
                Some(t) => w.append("\"").append(&self.rename(t)).append("\".").append(&column),
            };
            return Ok(());
        }
        if let SqlExpr::RowOrder { table, .. } = e {
            match table {
                None => w.append("rowid"),
                Some(t) => w.append("\"").append(&self.rename(t)).append("\".rowid"),
            };
            return Ok(());
        }
        if self.engine_date_diff(w, e)? {
            return Ok(());
        }
        // the engine's boolean text: lowercase keywords, AND flat, AND under OR parenthesized
        if let SqlExpr::Call { r#fn, args: a, .. } = e {
            match r#fn {
                SqlFn::NullSafeEqual | SqlFn::NullSafeNotEqual => {
                    let op =
                        if *r#fn == SqlFn::NullSafeEqual { " is not distinct from " } else { " is distinct from " };
                    w.expr(self, arg(a, 0)?, 4)?.append(op).expr(self, arg(a, 1)?, 4)?;
                    return Ok(());
                }
                SqlFn::And => {
                    let mut terms = Vec::new();
                    flatten_and(e, &mut terms);
                    w.join(&terms, " and ", |w, (t, or_like)| {
                        if *or_like {
                            w.append("(").expr(self, t, 0)?.append(")");
                        } else {
                            w.expr(self, t, 3)?;
                        }
                        Ok(())
                    })?;
                    return Ok(());
                }
                SqlFn::Or => {
                    w.join(a, " or ", |w, o| {
                        if matches!(o, SqlExpr::Call { r#fn: SqlFn::And, .. }) {
                            w.append("(").expr(self, o, 0)?.append(")");
                        } else {
                            w.expr(self, o, 0)?;
                        }
                        Ok(())
                    })?;
                    return Ok(());
                }
                // the null-guarded in() spells the bare in: every stacked COALESCE(x, false) guard unwrapped
                SqlFn::Coalesce => {
                    if a.len() == 2 && is_false(&a[1]) {
                        let mut in0 = &a[0];
                        while let SqlExpr::Call { r#fn: SqlFn::Coalesce, args: cc, .. } = in0
                            && cc.len() == 2
                            && is_false(&cc[1])
                        {
                            in0 = &cc[0];
                        }
                        if let SqlExpr::Call { r#fn: SqlFn::In, .. } = in0 {
                            return self.expr(w, in0, parent_prec);
                        }
                    }
                }
                SqlFn::In => return self.in_arm(w, a),
                SqlFn::IsNull => {
                    w.expr(self, arg(a, 0)?, 4)?.append(" is null");
                    return Ok(());
                }
                SqlFn::IsNotNull => {
                    w.expr(self, arg(a, 0)?, 4)?.append(" is not null");
                    return Ok(());
                }
                _ => {}
            }
        }
        ansi::expr(self, w, e, parent_prec)
    }

    /// The `IN` arm of `expr`: the temp-table splice, a collection parameter's template, a singleton as equality, the
    /// lowercase in-list.
    fn in_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2
            && let SqlExpr::TempTableInSplice { temp_table_name, .. } = &a[1]
        {
            let alias = temp_table_name.to_lowercase() + "_0";
            w.expr(self, &a[0], 4)?.append(" in (select \"").append(&alias);
            w.append("\".ColumnForStoringInCollection as ColumnForStoringInCollection from ").append(temp_table_name);
            w.append(" as \"").append(&alias).append("\")");
            return Ok(());
        }
        if a.len() == 2
            && let Some(cp) = PlanParam::of(&a[1])
        {
            // the temp-table protocol's wrapper variable: a bare splice
            if cp.kind == ParamKind::Raw {
                w.expr(self, &a[0], 4)?.append(" in (${").append(cp.name).append("})");
                return Ok(());
            }
            // a non-default zone's date-time collection: the TZ-shifting template, quotes unescaped
            if cp.kind == ParamKind::Datetime
                && let Some(tz) = &self.time_zone
            {
                w.expr(self, &a[0], 4)?.append(" in (${renderCollectionWithTz(").append(cp.name).append("![] \"[");
                w.append(tz).append("]\" \",\" \"TIMESTAMP'\" \"'\" \"null\")})");
                return Ok(());
            }
            w.expr(self, &a[0], 4)?.append(" in (");
            let splice = self.collection_splice(&cp)?;
            w.append(&splice).append(")");
            return Ok(());
        }
        // a singleton literal in-list is equality
        if a.len() == 2 {
            w.expr(self, &a[0], 4)?.append(" = ").expr(self, &a[1], 4)?;
            return Ok(());
        }
        w.expr(self, arg(a, 0)?, 4)?.append(" in (").list(self, tail(a, 1)?)?.append(")");
        Ok(())
    }

    /// A plan parameter: an optional one the `varPlaceHolderToString` template, any other the engine's placeholder
    /// for its kind.
    fn plan_param(&self, w: &mut SqlWriter, p: &PlanParam<'_>) -> Result<(), Refusal> {
        if p.optional {
            let holder = self.holder(p)?;
            w.append(&holder);
            return Ok(());
        }
        let name = p.name;
        match p.kind {
            ParamKind::Raw => w.append("${").append(name).append("}"),
            ParamKind::String => w.append("'${").append(name).append("?replace(\"'\", \"''\")}'"),
            ParamKind::Date => w.append("TIMESTAMP'${").append(name).append("}'"),
            ParamKind::StrictDate => w.append("DATE'${").append(name).append("}'"),
            ParamKind::Datetime => match &self.time_zone {
                Some(tz) => w.append("TIMESTAMP'${GMTtoTZ( \"[").append(tz).append("]\" ").append(name).append(")}'"),
                None => w.append("TIMESTAMP'${").append(name).append("}'"),
            },
            ParamKind::Enum => w.append("'${").append(name).append("}'"),
            ParamKind::Float | ParamKind::Boolean | ParamKind::Other => w.append("${").append(name).append("}"),
        };
        Ok(())
    }

    /// `${varPlaceHolderToString(name![] <prefix> <suffix> <escapes> "null")}`, a date-time in `GMTtoTZ` under a
    /// connection's zone.
    fn holder(&self, p: &PlanParam<'_>) -> Result<String, Refusal> {
        let mut inner = format!("{}![]", p.name);
        if p.kind == ParamKind::Datetime
            && let Some(tz) = &self.time_zone
        {
            inner = format!("GMTtoTZ( \"[{tz}]\" {inner})");
        }
        Ok(format!("${{varPlaceHolderToString({inner} {} \"null\")}}", self.holder_args(p.kind)?))
    }

    /// The template's prefix, suffix and escape map for a parameter's kind; DB2 quotes a boolean.
    fn holder_args(&self, k: ParamKind) -> Result<&'static str, Refusal> {
        if self.profile.db2() && k == ParamKind::Boolean {
            return Ok("\"'\" \"'\" {}");
        }
        Ok(match k {
            ParamKind::Raw => {
                return Err(Refusal::illegal_state("RAW plan params are bare splices, never collections"));
            }
            ParamKind::String => "\"'\" \"'\" {\"'\" : \"''\"}",
            // DATE and DATETIME both carry the TIMESTAMP keyword on the upgraded-H2 plan surface
            ParamKind::Date | ParamKind::Datetime => "\"TIMESTAMP'\" \"'\" {}",
            ParamKind::StrictDate => "\"DATE'\" \"'\" {}",
            ParamKind::Float => "\"CAST(\" \" AS FLOAT)\" {}",
            ParamKind::Boolean | ParamKind::Enum | ParamKind::Other => "\"\" \"\" {}",
        })
    }

    /// `rawColumn = enumParam` as the engine's `optionalVarPlaceHolderOperationSelector` template; none when the
    /// expression is not that shape. The column is written now, quoted into the template's arguments (`'` as `\'`).
    fn enum_selector(&self, e: &SqlExpr) -> Result<Option<String>, Refusal> {
        let SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } = e else {
            return Ok(None);
        };
        if args.len() != 2 {
            return Ok(None);
        }
        // the enum parameter may sit on either side
        let (p, other) = match (PlanParam::of(&args[1]), PlanParam::of(&args[0])) {
            (Some(p1), _) if p1.enum_map_fn.is_some() => (p1, &args[0]),
            (_, Some(p0)) if p0.enum_map_fn.is_some() => (p0, &args[1]),
            _ => return Ok(None),
        };
        // a required parameter against an enum literal: plain placeholder equality
        if matches!(other, SqlExpr::StringLit { .. }) && !p.optional {
            return Ok(None);
        }
        // the class property may arrive as the mapping's decode chain: the template compares its raw source
        let column = decode_shapes::source_expr(other).unwrap_or(other);
        let mut cw = SqlWriter::new();
        self.expr(&mut cw, column, 4)?;
        let col = cw.into_text()?.replace('\'', "\\'");
        let pn = format!("{}{}", p.name, if p.optional { "![]" } else { "" });
        let f = format!("{}({pn})", p.enum_map_fn.unwrap_or_default());
        Ok(Some(
            "(${optionalVarPlaceHolderOperationSelector(".to_owned()
                + &pn
                + ", equalEnumOperationSelector("
                + &f
                + ", '"
                + &col
                + " in (${"
                + &f
                + "})', '"
                + &col
                + " = ${"
                + &f
                + "}'), '0 = 1')})",
        ))
    }

    /// An optional plan parameter in an equality: null-safe on the plan surface (`nullSafeEq`), its side kept; false
    /// (nothing written) when the expression is not that shape.
    fn optional_param_equality(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<bool, Refusal> {
        let SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } = e else {
            return Ok(false);
        };
        if args.len() != 2 {
            return Ok(false);
        }
        let (l, r) = (&args[0], &args[1]);
        let lp = PlanParam::of(l).filter(|p| p.optional);
        let rp = PlanParam::of(r).filter(|p| p.optional);
        if let (Some(lp), Some(rp)) = (lp, rp) {
            self.null_safe_eq(w, &self.held(lp), &self.held(rp))?;
            return Ok(true);
        }
        let (opt, left_side) = match (lp, rp) {
            (Some(p), _) => (p, true),
            (None, Some(p)) => (p, false),
            (None, None) => return Ok(false),
        };
        let other = if left_side { r } else { l };
        let other_tx = |w: &mut SqlWriter| w.expr(self, other, 4).map(drop);
        let held = self.held(opt);
        if left_side {
            self.null_safe_eq(w, &held, &other_tx)?;
        } else {
            self.null_safe_eq(w, &other_tx, &held)?;
        }
        Ok(true)
    }

    /// The piece that writes a parameter's holder where it is written (Java's `w -> w.append(holder(p))`, the holder
    /// spelled lazily).
    fn held<'a>(&'a self, p: PlanParam<'a>) -> impl Fn(&mut SqlWriter) -> Result<(), Refusal> + 'a {
        move |w: &mut SqlWriter| {
            let h = self.holder(&p)?;
            w.append(&h);
            Ok(())
        }
    }

    /// Null-safe equality on the plan surface: H2's `is not distinct from`; DB2 (which has none) the OR form.
    fn null_safe_eq(
        &self,
        w: &mut SqlWriter,
        l: crate::writer::Piece<'_>,
        r: crate::writer::Piece<'_>,
    ) -> Result<(), Refusal> {
        if self.profile.db2() {
            w.append("(").piece(l)?.append(" = ").piece(r)?.append(" or (").piece(l)?.append(" is null and ");
            w.piece(r)?.append(" is null))");
            return Ok(());
        }
        w.piece(l)?.append(" is not distinct from ").piece(r)?;
        Ok(())
    }

    /// `exists (select 1 from ...)`: the engine's correlated exists, a literal 1 projected whatever the subquery's
    /// columns.
    fn correlated_exists(&self, w: &mut SqlWriter, xs: &SqlSelect) -> Result<(), Refusal> {
        w.append("exists (");
        let one = Projection { expr: crate::build::int_lit(1), alias: None, out: None };
        let probe = crate::build::with_projections(xs.clone(), vec![one])?;
        ansi::inline(self, w, &SqlQuery::Select(Box::new(probe)))?;
        w.append(")");
        Ok(())
    }

    /// A physical column's spelling: a declared-quoted one keeps its quotes, any other bare (quoted under
    /// `quoteIdentifiers`).
    fn phys_column(&self, origin: Option<lite_sql::Origin>, name: &str) -> String {
        if origin == Some(lite_sql::Origin::PhysicalQuoted) { self.delimited(name) } else { self.phys(name) }
    }

    fn phys(&self, name: &str) -> String {
        if self.quote_identifiers { format!("\"{name}\"") } else { name.to_owned() }
    }

    /// dateDiff's composite shapes folded back to the engine's `datediff(unit, a, b)`, the mapping's enum decode
    /// chain spelled flat, and the generic lowercase CASE; false (nothing written) for any other expression.
    fn engine_date_diff(&self, w: &mut SqlWriter, e: &SqlExpr) -> Result<bool, Refusal> {
        // truncated elapsed time: (epoch_ms(end) - epoch_ms(start)) // unitMs
        if let SqlExpr::Call { r#fn: SqlFn::IntDivide, args: div, .. } = e
            && div.len() == 2
            && let SqlExpr::IntLit { value: u, .. } = &div[1]
            && let SqlExpr::Call { r#fn: SqlFn::Minus, args: minus, .. } = &div[0]
            && minus.len() == 2
            && let SqlExpr::Call { r#fn: SqlFn::EpochMs, args: end, .. } = &minus[0]
            && let SqlExpr::Call { r#fn: SqlFn::EpochMs, args: start, .. } = &minus[1]
        {
            // Java's `(int) u.value()`: the long's low 32 bits
            let unit = match *u as i32 {
                3_600_000 => Some("hour"),
                60_000 => Some("minute"),
                1_000 => Some("second"),
                _ => None,
            };
            if let Some(unit) = unit {
                w.append("datediff(").append(unit).append(", ").expr(self, arg(start, 0)?, 0)?.append(", ");
                w.expr(self, arg(end, 0)?, 0)?.append(")");
                return Ok(true);
            }
        }
        // Sunday-boundary weeks: CASE WHEN date_diff('day', end, start) <= 0 THEN forward ELSE backward, both branches
        // the sundayIndex difference
        if let SqlExpr::Case { whens, otherwise: Some(otherwise), .. } = e
            && whens.len() == 1
            && let SqlExpr::Call { r#fn: SqlFn::LessEqual, args: le, .. } = &whens[0].condition
            && le.len() == 2
            && let SqlExpr::Call { r#fn: SqlFn::DateDiff, args: day_diff, .. } = &le[0]
            && matches!(arg(day_diff, 0)?, SqlExpr::DatePartLit { part: DatePart::Day, .. })
            && matches!(le[1], SqlExpr::IntLit { value: 0, .. })
            && is_sunday_index_difference(&whens[0].then)?
            && is_sunday_index_difference(otherwise)?
        {
            w.append("datediff(week, ").expr(self, arg(day_diff, 2)?, 0)?.append(", ");
            w.expr(self, arg(day_diff, 1)?, 0)?.append(")");
            return Ok(true);
        }
        // the mapping's enum decode chain: flat, `col in (a, b)` or `col = a` per branch, `else null`
        if let SqlExpr::Case { .. } = e
            && let Some(src) = decode_shapes::source_expr(e)
            && let Some(arms) = decode_shapes::flatten_decode(e)
        {
            w.append("case");
            for arm in arms {
                let mut values = Vec::new();
                collect_equality_rhs(&arm.condition, &mut values)?;
                w.append(" when ").expr(self, src, 4)?;
                if values.len() == 1 {
                    w.append(" = ").expr(self, values[0], 0)?;
                } else {
                    w.append(" in (");
                    self.list_refs(w, &values)?;
                    w.append(")");
                }
                w.append(" then ").expr(self, &arm.then, 0)?;
            }
            w.append(" else null end");
            return Ok(true);
        }
        if let SqlExpr::Case { whens, otherwise, .. } = e {
            w.append("case");
            for wh in whens {
                w.append(" when ").expr(self, &wh.condition, 0)?.append(" then ").expr(self, &wh.then, 0)?;
            }
            if let Some(o) = otherwise {
                w.append(" else ").expr(self, o, 0)?;
            }
            w.append(" end");
            return Ok(true);
        }
        Ok(false)
    }

    /// The expressions, comma-separated (`writer.list` over a list Java builds).
    fn list_refs(&self, w: &mut SqlWriter, es: &[&SqlExpr]) -> Result<(), Refusal> {
        w.join(es, ", ", |w, e| w.expr(self, e, 0).map(drop))?;
        Ok(())
    }

    /// `writer.function(name, args)` over a list Java builds.
    fn function_refs(&self, w: &mut SqlWriter, name: &str, es: &[&SqlExpr]) -> Result<(), Refusal> {
        w.append(name).append("(");
        self.list_refs(w, es)?;
        w.append(")");
        Ok(())
    }

    // ==================================================================
    // Calls: EngineStyleH2's, EngineStyleDB2's, EngineStyleComposite's
    // ==================================================================

    /// `EngineStyleH2.call`: each function as the engine's H2 text spells it, by name.
    fn h2_call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        use SqlFn as F;
        let plain = |w: &mut SqlWriter, name: &str| w.function(self, name, a).map(drop);
        let fixed = |w: &mut SqlWriter, text: &str| {
            w.append(text);
            Ok(())
        };
        match f {
            // a digest: rawtohex(hash('<algorithm>', x))
            F::Md5 => self.digest(w, "MD5", a),
            F::Sha1 => self.digest(w, "SHA-1", a),
            F::Sha256 => self.digest(w, "SHA-256", a),
            F::Cbrt => {
                w.append("power(").expr(self, arg(a, 0)?, 0)?.append(", 1.0/3)");
                Ok(())
            }
            F::Substring => plain(w, "substring"),
            F::Strpos => {
                w.append("locate(").expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::Ltrim => {
                w.append("regexp_replace(").expr(self, arg(a, 0)?, 0)?.append(", '^[ ]+', '')");
                Ok(())
            }
            F::Rtrim => {
                w.append("regexp_replace(").expr(self, arg(a, 0)?, 0)?.append(", '[ ]+$', '')");
                Ok(())
            }
            F::Lpad => self.pad_call(w, "legend_h2_extension_lpad", a),
            F::Rpad => self.pad_call(w, "legend_h2_extension_rpad", a),
            // n-ary concat: nested CONCATs splice into one flat concat
            F::Concat => self.concat_arm(w, a),
            F::DateDiff => {
                w.append("datediff(").append(engine_part(part(arg(a, 0)?)?)?).append(", ");
                w.expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
                Ok(())
            }
            // the engine's adjust: dateadd(unit, n, x), its unit lowercase
            F::AddInterval | F::AddIntervalTemporal => {
                w.append("dateadd(").append(db_unit_of(part(arg(a, 0)?)?)?).append(", ");
                w.expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
                Ok(())
            }
            F::ParseInt => {
                w.append("cast(").expr(self, arg(a, 0)?, 0)?.append(" as integer)");
                Ok(())
            }
            F::BoolToText => {
                w.append("cast(").expr(self, arg(a, 0)?, 0)?.append(" as varchar)");
                Ok(())
            }
            F::Chr => {
                w.append("char(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::Length => {
                w.append("char_length(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::ReverseString => {
                w.append("legend_h2_extension_reverse_string(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::SplitPart => plain(w, "legend_h2_extension_split_part"),
            F::Today => fixed(w, "cast(now() as date)"),
            F::Strftime => self.strftime_arm(w, a),
            F::Trim => {
                if a.len() == 1 {
                    w.append("trim(both from ").expr(self, &a[0], 0)?.append(")");
                    Ok(())
                } else {
                    plain(w, "trim")
                }
            }
            F::DateTruncDay => {
                w.append("cast(truncate(").expr(self, arg(a, 0)?, 0)?.append(") as date)");
                Ok(())
            }
            F::Greater => self.greater(w, a, parent_prec),
            F::Extract => self.extract_arm(w, a),
            F::DateTrunc => self.date_trunc(w, a),
            // parseDate: the engine's toTimestamp on H2, one fixed pattern
            F::ParseDate => {
                w.append("cast(parsedatetime(").expr(self, arg(a, 0)?, 0)?.append(
                    ", 'yyyy-MM-dd HH:mm:ss[.SSSSSSSSS][.SSSSSSSS][.SSSSSSS][.SSSSSS][.SSSSS][.SSSS][.SSS][.SS][.S]') \
                     as timestamp)",
                );
                Ok(())
            }
            F::Strptime => self.strptime_arm(w, a),
            // ---- the ANSI operators
            F::And
            | F::Or
            | F::Equal
            | F::NotEqual
            | F::Less
            | F::LessEqual
            | F::GreaterEqual
            | F::Plus
            | F::Minus
            | F::Times => ansi::infix(self, w, f, a, parent_prec),
            // ---- the shared spellings
            F::Not => ansi::not_prefix(self, w, a, parent_prec),
            F::Divide => ansi::divide_as_doubles(self, w, a),
            F::Mod => ansi::mod_floored(self, w, a),
            F::Rem => ansi::mod_truncated(self, w, a),
            F::Negate => ansi::negation(self, w, a),
            F::IsNull => ansi::is_null_test(self, w, a),
            F::IsNotNull => ansi::is_not_null_test(self, w, a),
            F::In => ansi::in_list(self, w, a, parent_prec),
            F::ConcatJoin => plain(w, "concat"),
            F::Pi => fixed(w, "pi()"),
            F::Ceiling => ansi::bigint_of(self, w, "ceil", a),
            F::Floor => ansi::bigint_of(self, w, "floor", a),
            F::Sign => ansi::bigint_of(self, w, "sign", a),
            F::Xor => ansi::xor_as_and_or(self, w, a, parent_prec),
            F::UcFirst => ansi::first_char_cased(self, w, "upper", a, parent_prec),
            F::LcFirst => ansi::first_char_cased(self, w, "lower", a, parent_prec),
            F::Guid => fixed(w, "CAST(uuid() AS VARCHAR)"),
            F::Random => fixed(w, "rand()"),
            F::Now => fixed(w, "now()"),
            F::CurrentUserFn => fixed(w, "current_user"),
            F::IsDistinctFrom | F::NullSafeNotEqual => ansi::distinct_from(self, w, a),
            F::NullSafeEqual => ansi::not_distinct_from(self, w, a),
            F::RoundHalfUp => plain(w, "ROUND"),
            F::FloatCanon => ansi::float_canon_by_text(self, w, a, parent_prec),
            F::FloatPrint => ansi::float_print_by_text(self, w, a, parent_prec),
            // ---- the hooks
            F::ListFilter
            | F::ListTransform
            | F::ListConcat
            | F::ListGet
            | F::ListPosition
            | F::ListZip
            | F::ListDistinct
            | F::ListAppend
            | F::ListSum
            | F::ListMin
            | F::ListMax
            | F::ListAvg
            | F::ListMedian
            | F::ListMode
            | F::ListSort
            | F::ListSortDesc
            | F::ListTail
            | F::ListInit
            | F::RangeFn
            | F::ListProduct
            | F::ListReduce
            | F::ListSlice
            | F::RepeatValue
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::ListReverse
            | F::Typeof => self.list_call(w, f, a),
            F::ListExists => self.list_exists(w, a),
            F::ListForAll => self.list_for_all(w, a),
            F::StructInsert => self.struct_insert(w, a),
            F::Unnest => self.unnest_projection(w, a),
            F::Round => self.round_half_even(w, a),
            F::BitAnd | F::BitOr | F::BitXor | F::BitShiftLeft | F::BitShiftRight => self.bit_op(w, f, a),
            F::PureSplitPart => self.split_part_call(w, a),
            F::Hash => self.hash_signed(w, a),
            F::AllDistinct => self.all_distinct(w, a),
            F::ToVariant => self.variant_construct(w, a),
            F::VariantElements => self.variant_elements(w, a),
            F::VariantGet => self.variant_get(w, a),
            // ---- the plain spellings: name(args)
            F::Abs => plain(w, "abs"),
            F::Coalesce => self.coalesce_arm(w, a),
            F::Upper => plain(w, "upper"),
            F::Lower => plain(w, "lower"),
            F::Greatest => plain(w, "greatest"),
            F::Least => plain(w, "least"),
            F::ListFlatten => plain(w, "flatten"),
            F::Sqrt => plain(w, "sqrt"),
            F::Exp => plain(w, "exp"),
            F::Ln => plain(w, "ln"),
            F::Log10 => plain(w, "log10"),
            F::Pow => plain(w, "power"),
            F::Sin => plain(w, "sin"),
            F::Cos => plain(w, "cos"),
            F::Tan => plain(w, "tan"),
            F::Asin => plain(w, "asin"),
            F::Acos => plain(w, "acos"),
            F::Atan => plain(w, "atan"),
            F::Atan2 => plain(w, "atan2"),
            F::Sinh => plain(w, "sinh"),
            F::Cosh => plain(w, "cosh"),
            F::Tanh => plain(w, "tanh"),
            F::RegexpExtract => plain(w, "regexp_extract"),
            F::RegexpExtractAll => plain(w, "regexp_extract_all"),
            F::RegexpReplace => plain(w, "regexp_replace"),
            F::RegexpFullMatch => plain(w, "regexp_full_match"),
            F::MapFromLists => plain(w, "map"),
            F::MapFromEntries => plain(w, "map_from_entries"),
            F::MapExtract => plain(w, "map_extract"),
            F::MapKeys => plain(w, "map_keys"),
            F::MapValues => plain(w, "map_values"),
            F::MapConcat => plain(w, "map_concat"),
            F::StartsWith => plain(w, "starts_with"),
            F::EndsWith => plain(w, "ends_with"),
            F::Matches => plain(w, "regexp_matches"),
            F::Left => plain(w, "left"),
            F::Right => plain(w, "right"),
            F::Replace => plain(w, "replace"),
            F::Translate => Err(Refusal::capability(
                "translate reached the engine text: legend-engine has no translate dynaFunction to spell",
            )),
            F::Split => plain(w, "string_split"),
            F::AsciiCode => plain(w, "ascii"),
            F::Levenshtein => plain(w, "levenshtein"),
            F::Format => plain(w, "printf"),
            F::MakeDate => plain(w, "make_date"),
            F::Timezone => plain(w, "timezone"),
            F::DateAtTime => Err(Refusal::capability(
                "a time of day: legend-engine's relational lowering refuses StrictTime, so its text has no spelling for \
                 one",
            )),
            F::EpochSeconds => plain(w, "epoch"),
            F::EpochNs | F::FromEpochNs => Err(Refusal::capability(
                "a nanosecond epoch: legend-engine's H2 text keeps microseconds (the Cypher island's nanosecond leg is \
                 DuckDB's)",
            )),
            F::EpochMs => plain(w, "epoch_ms"),
            F::FromEpochSeconds => plain(w, "to_timestamp"),
            F::Dayname => plain(w, "dayname"),
            F::Monthname => plain(w, "monthname"),
            F::Cot => plain(w, "cot"),
            F::Radians => plain(w, "radians"),
            F::Degrees => plain(w, "degrees"),
            F::RepeatStr => plain(w, "repeat"),
            F::JaroWinkler => plain(w, "jaro_winkler_similarity"),
            F::ListLength => plain(w, "len"),
            F::FloorRaw => plain(w, "floor"),
            F::JsonType => plain(w, "json_type"),
            F::JsonArrayLength => plain(w, "json_array_length"),
            F::JsonPretty => plain(w, "json_pretty"),
            // DuckDB's spellings, which the corpus reads this text on DuckDB with
            F::JsonMergePatch => crate::duckdb_spellings::json_merge_patch(self, w, a),
            F::BitNot => crate::duckdb_spellings::bit_not(self, w, a),
            F::MapEmpty => crate::duckdb_spellings::map_empty(w),
            F::EncodeBase64 => crate::duckdb_spellings::encode_base64(self, w, a),
            F::DecodeBase64 => crate::duckdb_spellings::decode_base64(self, w, a),
            F::MakeTimestamp => crate::duckdb_spellings::make_timestamp(self, w, a),
            F::FromEpochMs => crate::duckdb_spellings::from_epoch_ms(self, w, a),
            F::IntDivide => crate::duckdb_spellings::int_divide(self, w, a),
            F::TimeBucket => crate::duckdb_spellings::time_bucket(self, w, a),
            F::Error => ansi::raise_envelope(self, w, a, "error"),
        }
    }

    /// `EngineStyleDB2.call`: the forms DB2's goldens pin, every other function its parent's.
    fn db2_call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        use SqlFn as F;
        let one = |w: &mut SqlWriter, name: &str| -> Result<(), Refusal> {
            w.append(name).append("(").expr(self, arg(a, 0)?, 0)?.append(")");
            Ok(())
        };
        match f {
            // the infix operator, the whole chain parenthesized once: (a concat b concat c)
            F::Concat => {
                w.append("(");
                w.join(&flatten_concat(a), " concat ", |w, e| w.expr(self, e, 0).map(drop))?;
                w.append(")");
                Ok(())
            }
            // joinStrings' interleaved unit: the same chain, never spliced into an enclosing one
            F::ConcatJoin => {
                w.append("(").join_exprs(self, a, " concat ", 0)?.append(")");
                Ok(())
            }
            F::Trim if a.len() == 1 => one(w, "trim"),
            F::Cbrt => one(w, "cbrt"),
            F::Strpos => {
                w.append("position(").expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            // DB2's short substr (Composite keeps the full substring)
            F::Substring => w.function(self, "substr", a).map(drop),
            F::Ltrim => one(w, "ltrim"),
            F::Rtrim => one(w, "rtrim"),
            F::Lpad => w.function(self, "lpad", a).map(drop),
            F::Rpad => w.function(self, "rpad", a).map(drop),
            F::ReverseString => one(w, "reverse"),
            F::Extract => {
                if a.len() == 2 && part(&a[0])? == DatePart::DayOfYear {
                    w.append("dayofyear(").expr(self, &a[1], 0)?.append(")");
                    return Ok(());
                }
                self.h2_call(w, f, a, parent_prec)
            }
            // left/right wrapped in trim
            F::Left | F::Right => {
                let name = if f == F::Left { "trim(left(" } else { "trim(right(" };
                w.append(name).expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append("))");
                Ok(())
            }
            F::Today => {
                w.append("date(current date)");
                Ok(())
            }
            F::Length => {
                w.append("CHARACTER_LENGTH(").expr(self, arg(a, 0)?, 0)?.append(",CODEUNITS32)");
                Ok(())
            }
            F::DateTrunc => self.db2_date_trunc(w, a),
            F::AddInterval => self.db2_add_interval(w, a),
            F::Strptime => self.db2_strptime(w, a),
            // every other function: the parent's
            F::Trim
            | F::And
            | F::Or
            | F::Not
            | F::Equal
            | F::NotEqual
            | F::Less
            | F::LessEqual
            | F::Greater
            | F::GreaterEqual
            | F::Plus
            | F::Minus
            | F::Times
            | F::Divide
            | F::Mod
            | F::Rem
            | F::Negate
            | F::Abs
            | F::IsNull
            | F::IsNotNull
            | F::In
            | F::Coalesce
            | F::ParseInt
            | F::Upper
            | F::Lower
            | F::Greatest
            | F::Least
            | F::ListFilter
            | F::ListTransform
            | F::ListConcat
            | F::ListGet
            | F::ListPosition
            | F::ListExists
            | F::ListForAll
            | F::StructInsert
            | F::Unnest
            | F::ListFlatten
            | F::JsonMergePatch
            | F::Sqrt
            | F::Exp
            | F::Ln
            | F::Log10
            | F::Pow
            | F::Pi
            | F::Sin
            | F::Cos
            | F::Tan
            | F::Asin
            | F::Acos
            | F::Atan
            | F::Atan2
            | F::Sinh
            | F::Cosh
            | F::Tanh
            | F::Ceiling
            | F::Floor
            | F::Round
            | F::Sign
            | F::Xor
            | F::BitAnd
            | F::BitOr
            | F::BitXor
            | F::BitShiftLeft
            | F::BitShiftRight
            | F::BitNot
            | F::RegexpExtract
            | F::RegexpExtractAll
            | F::RegexpReplace
            | F::RegexpFullMatch
            | F::MapFromLists
            | F::MapFromEntries
            | F::MapEmpty
            | F::MapExtract
            | F::MapKeys
            | F::MapValues
            | F::MapConcat
            | F::StartsWith
            | F::EndsWith
            | F::Matches
            | F::Replace
            | F::Translate
            | F::Split
            | F::SplitPart
            | F::PureSplitPart
            | F::AsciiCode
            | F::Chr
            | F::UcFirst
            | F::LcFirst
            | F::EncodeBase64
            | F::Levenshtein
            | F::Guid
            | F::Random
            | F::Format
            | F::Hash
            | F::Md5
            | F::Sha1
            | F::Sha256
            | F::Now
            | F::DateTruncDay
            | F::MakeDate
            | F::MakeTimestamp
            | F::DateDiff
            | F::TimeBucket
            | F::Timezone
            | F::DateAtTime
            | F::AddIntervalTemporal
            | F::EpochSeconds
            | F::EpochMs
            | F::EpochNs
            | F::FromEpochNs
            | F::FromEpochSeconds
            | F::FromEpochMs
            | F::Dayname
            | F::Monthname
            | F::Cot
            | F::Radians
            | F::Degrees
            | F::IntDivide
            | F::RepeatStr
            | F::JaroWinkler
            | F::DecodeBase64
            | F::CurrentUserFn
            | F::ListLength
            | F::ListZip
            | F::ListDistinct
            | F::ListAppend
            | F::ListSum
            | F::ListMin
            | F::ListMax
            | F::ListAvg
            | F::ListMedian
            | F::ListMode
            | F::ListSort
            | F::ListSortDesc
            | F::ListTail
            | F::ListInit
            | F::RangeFn
            | F::ListProduct
            | F::ListReduce
            | F::ListSlice
            | F::RepeatValue
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::IsDistinctFrom
            | F::AllDistinct
            | F::NullSafeEqual
            | F::NullSafeNotEqual
            | F::Strftime
            | F::ParseDate
            | F::ListReverse
            | F::Typeof
            | F::RoundHalfUp
            | F::Error
            | F::FloorRaw
            | F::ToVariant
            | F::BoolToText
            | F::FloatCanon
            | F::FloatPrint
            | F::VariantElements
            | F::VariantGet
            | F::JsonType
            | F::JsonArrayLength
            | F::JsonPretty => self.h2_call(w, f, a, parent_prec),
        }
    }

    /// `EngineStyleComposite.call`: plain `char_length` and the full `substring`, every other function DB2's.
    fn composite_call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        use SqlFn as F;
        match f {
            F::Length => {
                w.append("char_length(").expr(self, arg(a, 0)?, 0)?.append(")");
                Ok(())
            }
            F::Substring => w.function(self, "substring", a).map(drop),
            F::And
            | F::Or
            | F::Not
            | F::Equal
            | F::NotEqual
            | F::Less
            | F::LessEqual
            | F::Greater
            | F::GreaterEqual
            | F::Plus
            | F::Minus
            | F::Times
            | F::Divide
            | F::Mod
            | F::Rem
            | F::Negate
            | F::Abs
            | F::IsNull
            | F::IsNotNull
            | F::In
            | F::Coalesce
            | F::ParseInt
            | F::Concat
            | F::Upper
            | F::Lower
            | F::Greatest
            | F::Least
            | F::ConcatJoin
            | F::ListFilter
            | F::ListTransform
            | F::ListConcat
            | F::ListGet
            | F::ListPosition
            | F::ListExists
            | F::ListForAll
            | F::StructInsert
            | F::Unnest
            | F::ListFlatten
            | F::JsonMergePatch
            | F::Sqrt
            | F::Cbrt
            | F::Exp
            | F::Ln
            | F::Log10
            | F::Pow
            | F::Pi
            | F::Sin
            | F::Cos
            | F::Tan
            | F::Asin
            | F::Acos
            | F::Atan
            | F::Atan2
            | F::Sinh
            | F::Cosh
            | F::Tanh
            | F::Ceiling
            | F::Floor
            | F::Round
            | F::Sign
            | F::Xor
            | F::BitAnd
            | F::BitOr
            | F::BitXor
            | F::BitShiftLeft
            | F::BitShiftRight
            | F::BitNot
            | F::RegexpExtract
            | F::RegexpExtractAll
            | F::RegexpReplace
            | F::RegexpFullMatch
            | F::MapFromLists
            | F::MapFromEntries
            | F::MapEmpty
            | F::MapExtract
            | F::MapKeys
            | F::MapValues
            | F::MapConcat
            | F::Strpos
            | F::StartsWith
            | F::EndsWith
            | F::Matches
            | F::Left
            | F::Right
            | F::Lpad
            | F::Rpad
            | F::Trim
            | F::Ltrim
            | F::Rtrim
            | F::Replace
            | F::Translate
            | F::Split
            | F::SplitPart
            | F::PureSplitPart
            | F::ReverseString
            | F::AsciiCode
            | F::Chr
            | F::UcFirst
            | F::LcFirst
            | F::EncodeBase64
            | F::Levenshtein
            | F::Guid
            | F::Random
            | F::Format
            | F::Hash
            | F::Md5
            | F::Sha1
            | F::Sha256
            | F::Extract
            | F::Today
            | F::Now
            | F::DateTruncDay
            | F::MakeDate
            | F::MakeTimestamp
            | F::DateTrunc
            | F::AddInterval
            | F::DateDiff
            | F::TimeBucket
            | F::Timezone
            | F::DateAtTime
            | F::AddIntervalTemporal
            | F::EpochSeconds
            | F::EpochMs
            | F::EpochNs
            | F::FromEpochNs
            | F::FromEpochSeconds
            | F::FromEpochMs
            | F::Dayname
            | F::Monthname
            | F::Cot
            | F::Radians
            | F::Degrees
            | F::IntDivide
            | F::RepeatStr
            | F::JaroWinkler
            | F::DecodeBase64
            | F::CurrentUserFn
            | F::ListLength
            | F::ListZip
            | F::ListDistinct
            | F::ListAppend
            | F::ListSum
            | F::ListMin
            | F::ListMax
            | F::ListAvg
            | F::ListMedian
            | F::ListMode
            | F::ListSort
            | F::ListSortDesc
            | F::ListTail
            | F::ListInit
            | F::RangeFn
            | F::ListProduct
            | F::ListReduce
            | F::ListSlice
            | F::RepeatValue
            | F::ListBoolAnd
            | F::ListBoolOr
            | F::IsDistinctFrom
            | F::AllDistinct
            | F::NullSafeEqual
            | F::NullSafeNotEqual
            | F::Strftime
            | F::Strptime
            | F::ParseDate
            | F::ListReverse
            | F::Typeof
            | F::RoundHalfUp
            | F::Error
            | F::FloorRaw
            | F::ToVariant
            | F::BoolToText
            | F::FloatCanon
            | F::FloatPrint
            | F::VariantElements
            | F::VariantGet
            | F::JsonType
            | F::JsonArrayLength
            | F::JsonPretty => self.db2_call(w, f, a, parent_prec),
        }
    }

    /// `rawtohex(hash('<algorithm>', x))`.
    fn digest(&self, w: &mut SqlWriter, algorithm: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("rawtohex(hash('").append(algorithm).append("', ").expr(self, arg(a, 0)?, 0)?.append("))");
        Ok(())
    }

    /// `name(s, n, pad)`: the engine's pad extension; Pure's pad character is optional, a space.
    fn pad_call(&self, w: &mut SqlWriter, name: &str, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append(name).append("(").expr(self, arg(a, 0)?, 0)?.append(", ").expr(self, arg(a, 1)?, 0)?.append(", ");
        if a.len() > 2 {
            w.expr(self, &a[2], 0)?;
        } else {
            w.append("' '");
        }
        w.append(")");
        Ok(())
    }

    /// The CONCAT arm: joinStrings over a literal list as one flat `concat(elements..., separators...)` (the 4-argument
    /// forms; the 1- and 2-argument ones are COALESCE's), any other concat spliced flat.
    fn concat_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2 {
            // CONCAT(CONCAT(prefix, J), suffix)
            if let SqlExpr::Call { r#fn: SqlFn::Concat, args: c0, .. } = &a[0]
                && c0.len() == 2
                && let Some((collection, extras)) = joined_reduction(&c0[1])?
            {
                return self.flat_concat(w, collection, &[&c0[0], &extras[0], &a[1]]);
            }
            // CONCAT(prefix, CONCAT(J, suffix))
            if let SqlExpr::Call { r#fn: SqlFn::Concat, args: c1, .. } = &a[1]
                && c1.len() == 2
                && let Some((collection, extras)) = joined_reduction(&c1[0])?
            {
                return self.flat_concat(w, collection, &[&a[0], &extras[0], &c1[1]]);
            }
        }
        self.function_refs(w, "concat", &flatten_concat(a))
    }

    /// The COALESCE arm: joinStrings' 1- and 2-argument forms (an empty separator is the 1-argument desugar, three
    /// `''`), an optional collection parameter's size as the engine's `collectionSize` template, else `coalesce`.
    fn coalesce_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if let Some((collection, extras)) = joined_reduction_of(a)? {
            let sep = &extras[0];
            let empty = matches!(sep, SqlExpr::StringLit { value, .. } if value.is_empty());
            let seps: Vec<&SqlExpr> = if empty { vec![sep, sep, sep] } else { vec![sep] };
            return self.flat_concat(w, collection, &seps);
        }
        if a.len() == 2
            && let SqlExpr::Call { r#fn: SqlFn::ListLength, args: len, .. } = &a[0]
            && len.len() == 1
            && let Some(cp) = PlanParam::of(&len[0])
            && matches!(a[1], SqlExpr::IntLit { value: 0, .. })
        {
            w.append("(${collectionSize(").append(cp.name).append("![])})");
            return Ok(());
        }
        w.function(self, "coalesce", a)?;
        Ok(())
    }

    /// `concat(el1, el2, ..., separators...)`: each element's text coercion dropped.
    fn flat_concat(&self, w: &mut SqlWriter, collection: &SqlExpr, seps: &[&SqlExpr]) -> Result<(), Refusal> {
        let elements = literal_elements(collection)?.unwrap_or_default();
        let mut parts: Vec<&SqlExpr> = elements.iter().map(unwrap_element_text).collect();
        parts.extend_from_slice(seps);
        self.function_refs(w, "concat", &parts)
    }

    /// `greatest(a, b, c)` / `least(...)`, a literal's coercion cast dropped; `greatest(null)` for none.
    fn variadic_extreme(&self, w: &mut SqlWriter, max: bool, elems: &[SqlExpr]) -> Result<(), Refusal> {
        let name = if max { "greatest" } else { "least" };
        if elems.is_empty() {
            w.append(name).append("(null)");
            return Ok(());
        }
        w.append(name).append("(");
        w.join(elems, ", ", |w, e| w.expr(self, uncast_literal(e), 0).map(drop))?;
        w.append(")");
        Ok(())
    }

    /// The STRFTIME arm: the engine's `formatdatetime` for a month or weekday name; any other format refused.
    fn strftime_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2
            && let SqlExpr::FormatLit { parts, .. } = &a[1]
            && parts.len() == 1
            && let DateFmt::Part(p) = &parts[0]
        {
            let java = match p {
                DateFmtPart::MonthName => Some("MMMM"),
                DateFmtPart::WeekdayName => Some("EEEE"),
                _ => None,
            };
            if let Some(java) = java {
                w.append("formatdatetime(").expr(self, &a[0], 0)?.append(", '").append(java).append("')");
                return Ok(());
            }
        }
        Err(Refusal::illegal_state(format!(
            "strftime format has no engine-H2 formatdatetime spelling yet: {}",
            list_text(a)
        )))
    }

    /// The GREATER arm: `strpos(x, 'lit') > 0` as the engine's `x like '%lit%'`.
    fn greater(&self, w: &mut SqlWriter, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        if a.len() == 2
            && let SqlExpr::Call { r#fn: SqlFn::Strpos, args: sp, .. } = &a[0]
            && sp.len() == 2
            && let SqlExpr::StringLit { value: lit, .. } = &sp[1]
            && matches!(a[1], SqlExpr::IntLit { value: 0, .. })
        {
            w.expr(self, &sp[0], 0)?.append(" like '%").append(&lit.replace('\'', "''")).append("%'");
            return Ok(());
        }
        ansi::infix(self, w, SqlFn::Greater, a, parent_prec)
    }

    /// The EXTRACT arm: `extract(doy from x)`, any other part `date_part('<part>', x)`.
    fn extract_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2 && part(&a[0])? == DatePart::DayOfYear {
            w.append("extract(doy from ").expr(self, &a[1], 0)?.append(")");
            return Ok(());
        }
        let spelled = self.string_lit(engine_part(part(arg(a, 0)?)?)?);
        w.append("date_part(").append(&spelled).append(", ").list(self, tail(a, 1)?)?.append(")");
        Ok(())
    }

    /// The DATE_TRUNC arm: week to year the engine's double cast (a TODAY anchor bare `now()`), finer parts the
    /// day-grained truncation.
    fn date_trunc(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        let u = part(arg(a, 0)?)?;
        if a.len() == 2 && matches!(u, DatePart::Week | DatePart::Month | DatePart::Quarter | DatePart::Year) {
            w.append("cast(cast(date_trunc('").append(engine_part(u)?).append("', ");
            if matches!(a[1], SqlExpr::Call { r#fn: SqlFn::Today, .. }) {
                w.append("now()");
            } else {
                w.expr(self, &a[1], 0)?;
            }
            w.append(") as timestamp) as date)");
            return Ok(());
        }
        let spelled = self.string_lit(engine_part(u)?);
        ansi::date_trunc_date_grained(self, w, a, &spelled)
    }

    /// The STRPTIME arm: the engine's `parsedatetime` with the format's Java pattern (a date-only format over the
    /// first ten characters; `MMMyyyy` with a `01` day prepended); any other format refused.
    fn strptime_arm(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2
            && let SqlExpr::FormatLit { parts, .. } = &a[1]
            && let Some(java) = h2_pattern(parts)
        {
            let date_only = !parts.contains(&DateFmt::Part(DateFmtPart::Hour2));
            if java == "MMMyyyy" {
                w.append("parsedatetime(concat('01', ").expr(self, &a[0], 0)?.append("), 'ddMMMyyyy')");
            } else if date_only {
                w.append("parsedatetime(substring(").expr(self, &a[0], 0)?.append(", 1, 10), '").append(&java);
                w.append("')");
            } else {
                w.append("parsedatetime(").expr(self, &a[0], 0)?.append(", '").append(&java).append("')");
            }
            return Ok(());
        }
        Err(Refusal::illegal_state(format!(
            "strptime format has no engine-H2 parsedatetime spelling yet: {}",
            list_text(a)
        )))
    }

    /// DB2's DATE_TRUNC: the truncation rebuilt from `date(1)` with labeled year and month arithmetic.
    fn db2_date_trunc(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2 {
            let anchor = |w: &mut SqlWriter| -> Result<(), Refusal> {
                if matches!(a[1], SqlExpr::Call { r#fn: SqlFn::Today, .. }) {
                    w.append("current date");
                } else {
                    w.expr(self, &a[1], 0)?;
                }
                Ok(())
            };
            match part(&a[0])? {
                DatePart::Year => {
                    w.append("date(1) + (year(").piece(&anchor)?.append(")-1) YEARS");
                    return Ok(());
                }
                DatePart::Month => {
                    w.append("date(1) + (year(").piece(&anchor)?.append(")-1) YEARS + (month(").piece(&anchor)?;
                    w.append(")-1) MONTHS");
                    return Ok(());
                }
                DatePart::Quarter => {
                    w.append("date(1) + ((year(").piece(&anchor)?.append(")-1) YEARS) + (3 * QUARTER(");
                    w.piece(&anchor)?.append(") - 3) MONTHS");
                    return Ok(());
                }
                DatePart::Week
                | DatePart::Day
                | DatePart::Hour
                | DatePart::Minute
                | DatePart::Second
                | DatePart::Millisecond
                | DatePart::Microsecond
                | DatePart::Nanosecond
                | DatePart::DayOfYear
                | DatePart::IsoDayOfWeek => {}
            }
        }
        Err(Refusal::illegal_state(format!("date_trunc unit has no engine-DB2 spelling yet: {}", list_text(a))))
    }

    /// DB2's ADD_INTERVAL over a literal count: `x - 1 MONTHS`, `x + 3 DAYS` (a week as seven days).
    fn db2_add_interval(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 3
            && let SqlExpr::IntLit { value: n, .. } = &a[1]
        {
            let u = part(&a[0])?;
            let mut unit = match u {
                DatePart::Year => Some("YEARS"),
                DatePart::Month => Some("MONTHS"),
                DatePart::Day => Some("DAYS"),
                DatePart::Hour => Some("HOURS"),
                DatePart::Minute => Some("MINUTES"),
                DatePart::Second => Some("SECONDS"),
                DatePart::Quarter
                | DatePart::Week
                | DatePart::Millisecond
                | DatePart::Microsecond
                | DatePart::Nanosecond
                | DatePart::DayOfYear
                | DatePart::IsoDayOfWeek => None,
            };
            let mut v = *n;
            if unit.is_none() && u == DatePart::Week {
                unit = Some("DAYS");
                v = v.wrapping_mul(7);
            }
            if let Some(unit) = unit {
                let count = if v < 0 { format!(" - {}", v.wrapping_neg()) } else { format!(" + {v}") };
                w.expr(self, &a[2], 0)?.append(&count).append(" ").append(unit);
                return Ok(());
            }
        }
        Err(Refusal::illegal_state(format!("interval shape has no engine-DB2 spelling yet: {}", list_text(a))))
    }

    /// DB2's STRPTIME: `to_date` (a date-only format) or `timestamp_format`, the whole string, the pattern after a
    /// bare comma.
    fn db2_strptime(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        if a.len() == 2
            && let SqlExpr::FormatLit { parts, .. } = &a[1]
            && let Some(java) = db2_pattern(parts)
        {
            let date_only = !parts.contains(&DateFmt::Part(DateFmtPart::Hour2));
            w.append(if date_only { "to_date(" } else { "timestamp_format(" }).expr(self, &a[0], 0)?.append(",'");
            w.append(&java).append("')");
            return Ok(());
        }
        Err(Refusal::illegal_state(format!("strptime format has no engine-DB2 spelling yet: {}", list_text(a))))
    }

    // ==================================================================
    // Aggregates
    // ==================================================================

    /// A percentile the engine writes within group: `name(p) within group (order by v asc|desc)`.
    fn percentile(&self, w: &mut SqlWriter, name: &str, r: &Reducer) -> Result<(), Refusal> {
        let desc = r.order_by.first().is_some_and(|k| !k.ascending);
        w.append(name).append("(").expr(self, arg(&r.args, 1)?, 0)?.append(") within group (order by ");
        w.expr(self, arg(&r.args, 0)?, 0)?.append(if desc { " desc" } else { " asc" }).append(")");
        Ok(())
    }

    /// `listagg([distinct] x, separator)`.
    fn listagg(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let distinct = if r.distinct { self.keyword("DISTINCT") + " " } else { String::new() };
        w.append("listagg(").append(&distinct).list(self, &r.args)?.append(")");
        Ok(())
    }

    /// ` within group (order by ...)`: an ordered string aggregate's ordering.
    fn within_group(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let order_by = self.keyword("ORDER BY");
        w.append(" within group (").append(&order_by).append(" ");
        w.join(&r.order_by, ", ", |w, k| ansi::aggregate_order_key(self, w, k))?;
        w.append(")");
        Ok(())
    }

    // ==================================================================
    // DML
    // ==================================================================

    /// A seed row as legend-engine's H2 `loadValuesToDbTableH2` writes it: one row a statement, a cell a column.
    fn seed_row(&self, dml: &SqlDml) -> Result<String, Refusal> {
        let SqlDml::InsertValues { schema, table, columns, quoted_columns, rows, .. } = dml else {
            return Err(Refusal::illegal_state("a seed row is an INSERT of values"));
        };
        if rows.len() != 1 || columns.len() != rows[0].len() {
            return Err(Refusal::capability(format!(
                "the engine's seed text writes one row a statement, a cell a column: {} rows of {} columns reached it",
                rows.len(),
                columns.len()
            )));
        }
        let names: Vec<String> =
            columns.iter().map(|c| AnsiSqlRenderer::insert_column_name(self, c, quoted_columns.contains(c))).collect();
        let cells = rows[0].iter().map(seed_cell).collect::<Result<Vec<_>, _>>()?;
        let schema = schema.as_ref().map_or(String::new(), |s| format!("{s}."));
        Ok(format!("insert into {schema}{table} ({}) values ({});", names.join(","), cells.join(",")))
    }
}

/// A cell as the engine's `convertValuesToCsvH2` writes it: `null`, a string, date or timestamp quoted (its quotes
/// doubled), a bit's truth, a decimal in plain notation; any other node refused.
fn seed_cell(cell: &SqlExpr) -> Result<String, Refusal> {
    Ok(match cell {
        SqlExpr::NullLit { .. } => "null".to_owned(),
        SqlExpr::StringLit { value, .. } => format!("'{}'", value.replace('\'', "''")),
        SqlExpr::DateLit { iso, .. } | SqlExpr::TimestampLit { iso, .. } => format!("'{}'", iso.replace('\'', "''")),
        SqlExpr::BoolLit { value, .. } => if *value { "true" } else { "false" }.to_owned(),
        SqlExpr::DecimalLit { value, .. } => big_decimal_plain(value.as_str()),
        other => {
            return Err(Refusal::capability(format!(
                "a {} cell reached the engine's seed text, which writes values only",
                simple_name(other)
            )));
        }
    })
}

/// The engine's column rule for H2 DDL text: a reserved word (the engine's H2 list), a pre-quoted or a space-bearing
/// name quoted (its quotes dropped), everything else bare.
fn engine_column_name(name: &str) -> String {
    if name.starts_with('"') || ENGINE_RESERVED.contains(&name.to_lowercase().as_str()) || name.contains(' ') {
        return format!("\"{}\"", name.replace('"', ""));
    }
    name.to_owned()
}

/// Java's `DdlSpelling.engineText`: the engine's `dataTypeToSqlText` (Integer `INT`, Other `OTHER`).
pub fn ddl_engine_text(t: &ColumnType) -> Result<String, Refusal> {
    Ok(match t {
        ColumnType::Plain { kind } => match kind {
            ColumnKind::Bigint => "BIGINT",
            ColumnKind::Smallint => "SMALLINT",
            ColumnKind::Tinyint => "TINYINT",
            ColumnKind::Integer => "INT",
            ColumnKind::Float => "FLOAT",
            ColumnKind::Double => "DOUBLE",
            ColumnKind::Real => "REAL",
            ColumnKind::Bit => "BIT",
            ColumnKind::Timestamp => "TIMESTAMP",
            ColumnKind::Date => "DATE",
            ColumnKind::Time => "TIME",
            ColumnKind::TimestampNs => "TIMESTAMP_NS",
            ColumnKind::Json => "JSON",
            ColumnKind::Varchar => "VARCHAR",
            ColumnKind::Other => "OTHER",
            ColumnKind::Distinct | ColumnKind::Array | ColumnKind::Object => {
                return Err(Refusal::illegal_state(format!("no DDL spelling for declared column type {kind}")));
            }
        }
        .to_owned(),
        ColumnType::Sized { kind, size } => format!("{kind}({size})"),
        ColumnType::Scaled { kind, precision, scale } => format!("{kind}({precision}, {scale})"),
    })
}

/// A DurationUnit's part as H2's dateadd names it, lowercase.
fn db_unit_of(unit: DatePart) -> Result<&'static str, Refusal> {
    Ok(match unit {
        DatePart::Year => "year",
        DatePart::Month => "month",
        DatePart::Week => "week",
        DatePart::Day => "day",
        DatePart::Hour => "hour",
        DatePart::Minute => "minute",
        DatePart::Second => "second",
        DatePart::Millisecond => "millisecond",
        DatePart::Microsecond => "microsecond",
        DatePart::Quarter | DatePart::DayOfYear | DatePart::IsoDayOfWeek => {
            return Err(Refusal::illegal_state(format!("no H2 dateadd unit for an interval of {unit}")));
        }
        DatePart::Nanosecond => {
            return Err(Refusal::capability(
                "a nanosecond interval: legend-engine's text keeps microseconds (the Cypher island's nanosecond leg is \
                 DuckDB's)",
            ));
        }
    })
}

/// A date part as the engine text names it: `datediff`'s, `extract`'s, `date_part`'s and `date_trunc`'s unit.
fn engine_part(p: DatePart) -> Result<&'static str, Refusal> {
    Ok(match p {
        DatePart::Year => "year",
        DatePart::Quarter => "quarter",
        DatePart::Month => "month",
        DatePart::Week => "week",
        DatePart::Day => "day",
        DatePart::Hour => "hour",
        DatePart::Minute => "minute",
        DatePart::Second => "second",
        DatePart::Millisecond => "millisecond",
        DatePart::Microsecond => "microsecond",
        DatePart::Nanosecond => {
            return Err(Refusal::capability(
                "a nanosecond part: legend-engine's text keeps microseconds (the Cypher island's nanosecond leg is \
                 DuckDB's)",
            ));
        }
        DatePart::DayOfYear => "doy",
        DatePart::IsoDayOfWeek => "isodow",
    })
}

/// A format's parts as the Java pattern the engine's H2 `parsedatetime` takes; none when a part has no mapping.
fn h2_pattern(parts: &[DateFmt]) -> Option<String> {
    let mut out = String::new();
    for d in parts {
        match d {
            DateFmt::Text { s } => out.push_str(s),
            DateFmt::Part(p) => out.push_str(match p {
                DateFmtPart::Year4 => "yyyy",
                DateFmtPart::Month2 => "MM",
                DateFmtPart::MonthAbbrev => "MMM",
                DateFmtPart::Day2 => "dd",
                DateFmtPart::Hour2 => "HH",
                DateFmtPart::Min2 => "mm",
                DateFmtPart::Sec2 => "ss",
                _ => return None,
            }),
        }
    }
    Some(out)
}

/// A format's parts as DB2's pattern; none when a part has no mapping.
fn db2_pattern(parts: &[DateFmt]) -> Option<String> {
    let mut out = String::new();
    for d in parts {
        match d {
            DateFmt::Text { s } => out.push_str(s),
            DateFmt::Part(p) => out.push_str(match p {
                DateFmtPart::Year4 => "yyyy",
                DateFmtPart::Month2 => "MM",
                DateFmtPart::Day2 => "dd",
                DateFmtPart::Hour2 => "hh",
                DateFmtPart::Min2 => "mm",
                DateFmtPart::Sec2 => "ss",
                DateFmtPart::SubsecMin => "mmm",
                _ => return None,
            }),
        }
    }
    Some(out)
}

/// Java's `List.toString()` of the arguments, each element's text as [`expr_text`] writes it (Java's is the whole
/// record's `toString`).
fn list_text(a: &[SqlExpr]) -> String {
    format!("[{}]", a.iter().map(expr_text).collect::<Vec<_>>().join(", "))
}

/// Java's `Projection.outputName()`: the alias, else a bare column's or a stored read's name.
fn output_name(expr: &SqlExpr, alias: Option<&str>) -> Option<String> {
    match (alias, expr) {
        (Some(a), _) => Some(a.to_owned()),
        (None, SqlExpr::Column(c)) => Some(c.name.clone()),
        (None, SqlExpr::StoredRead { column, .. }) => Some(column.name.clone()),
        _ => None,
    }
}

fn strip_quotes(n: &str) -> &str {
    if n.len() > 1 && n.starts_with('"') && n.ends_with('"') { &n[1..n.len() - 1] } else { n }
}

fn is_false(e: &SqlExpr) -> bool {
    matches!(e, SqlExpr::BoolLit { value: false, .. })
}

/// An arithmetic operation with a mixed-operator composite operand wraps itself (`((1.0*q)/basis)*rate`); a
/// same-operator chain stays flat; `parent_prec` 7 and up the base already wraps.
fn mixed_operand_arithmetic(f: SqlFn, args: &[SqlExpr], parent_prec: i32) -> bool {
    parent_prec < 7
        && matches!(f, SqlFn::Plus | SqlFn::Minus | SqlFn::Times)
        && args.iter().any(|x| {
            let u = match x {
                SqlExpr::Group { inner, .. } => inner.as_ref(),
                other => other,
            };
            matches!(u, SqlExpr::Call { r#fn: cf, .. }
                if *cf != f && matches!(cf, SqlFn::Plus | SqlFn::Minus | SqlFn::Times | SqlFn::Divide))
        })
}

/// The flattened conjuncts of an AND, each with whether it is an OR (parenthesized under the AND).
fn flatten_and<'e>(e: &'e SqlExpr, out: &mut Vec<(&'e SqlExpr, bool)>) {
    if let SqlExpr::Call { r#fn: SqlFn::And, args, .. } = e {
        for a in args {
            flatten_and(a, out);
        }
        return;
    }
    out.push((e, matches!(e, SqlExpr::Call { r#fn: SqlFn::Or, .. })));
}

/// An OR-rooted conjunct (DB2's WHERE wrap): an OR or a null-safe node, or an equality over an optional parameter
/// (expanded null-safe at render time); a group unwrapped.
fn or_rooted(a: &SqlExpr) -> bool {
    match a {
        SqlExpr::Group { inner, .. } => or_rooted(inner),
        SqlExpr::Call { r#fn: SqlFn::Or | SqlFn::NullSafeEqual | SqlFn::NullSafeNotEqual, .. } => true,
        SqlExpr::Call { r#fn: SqlFn::Equal | SqlFn::NotEqual, args, .. } => {
            args.iter().any(|x| PlanParam::of(x).is_some_and(|p| p.optional))
        }
        _ => false,
    }
}

/// The right-hand literals of a decode branch's equality, or of its OR-tree of equalities.
fn collect_equality_rhs<'e>(cond: &'e SqlExpr, out: &mut Vec<&'e SqlExpr>) -> Result<(), Refusal> {
    match cond {
        SqlExpr::Call { r#fn: SqlFn::Equal, args, .. } if args.len() == 2 => out.push(&args[1]),
        SqlExpr::Call { r#fn: SqlFn::Or, args, .. } => {
            for a in args {
                collect_equality_rhs(a, out)?;
            }
        }
        other => {
            return Err(Refusal::illegal_state(format!("decode branch is not an equality: {}", expr_text(other))));
        }
    }
    Ok(())
}

/// `sundayIndex(d2) - sundayIndex(d1)`, sundayIndex = `date_diff('day', DATE '0001-01-07', d) // 7`.
fn is_sunday_index_difference(e: &SqlExpr) -> Result<bool, Refusal> {
    let SqlExpr::Call { r#fn: SqlFn::Minus, args, .. } = e else {
        return Ok(false);
    };
    Ok(args.len() == 2 && is_sunday_index(&args[0])? && is_sunday_index(&args[1])?)
}

fn is_sunday_index(e: &SqlExpr) -> Result<bool, Refusal> {
    if let SqlExpr::Call { r#fn: SqlFn::IntDivide, args: div, .. } = e
        && div.len() == 2
        && matches!(div[1], SqlExpr::IntLit { value: 7, .. })
        && let SqlExpr::Call { r#fn: SqlFn::DateDiff, args: dd, .. } = &div[0]
    {
        return Ok(matches!(arg(dd, 1)?, SqlExpr::DateLit { iso, .. } if iso == "0001-01-07"));
    }
    Ok(false)
}

/// The empty collection: NULL, bare or cast to an array.
fn empty_collection(e: &SqlExpr) -> bool {
    match e {
        SqlExpr::NullLit { .. } => true,
        SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } => matches!(value.as_ref(), SqlExpr::NullLit { .. }),
        _ => false,
    }
}

/// A literal's coercion cast is noise in the engine text: the integer or float literal itself.
fn uncast_literal(e: &SqlExpr) -> &SqlExpr {
    match e {
        SqlExpr::Cast { value, .. } if matches!(value.as_ref(), SqlExpr::IntLit { .. } | SqlExpr::FloatLit { .. }) => {
            value
        }
        other => other,
    }
}

/// `x is not null` in any of its lowered spellings over the lambda's own parameter.
fn is_not_null_of(body: &SqlExpr, param: &str) -> bool {
    let SqlExpr::Call { r#fn, args: a, .. } = body else {
        return false;
    };
    match r#fn {
        SqlFn::IsNotNull => a.len() == 1 && is_param(&a[0], param),
        SqlFn::Not => {
            a.len() == 1
                && matches!(&a[0], SqlExpr::Call { r#fn: SqlFn::IsNull, args: inner, .. }
                    if inner.len() == 1 && is_param(&inner[0], param))
        }
        SqlFn::NotEqual | SqlFn::NullSafeNotEqual => {
            a.len() == 2
                && ((is_param(&a[0], param) && matches!(a[1], SqlExpr::NullLit { .. }))
                    || (is_param(&a[1], param) && matches!(a[0], SqlExpr::NullLit { .. })))
        }
        _ => false,
    }
}

fn is_param(e: &SqlExpr, param: &str) -> bool {
    matches!(e, SqlExpr::Column(c) if c.table.is_none() && c.name == param)
}

/// `COALESCE(ReduceCollection(STRING_AGG, <literal list>, [sep]), x)`: the collection and its extras; none for any
/// other shape.
fn joined_reduction(e: &SqlExpr) -> Result<Option<(&SqlExpr, &[SqlExpr])>, Refusal> {
    match e {
        SqlExpr::Call { r#fn: SqlFn::Coalesce, args, .. } => joined_reduction_of(args),
        _ => Ok(None),
    }
}

/// [`joined_reduction`] of a COALESCE's arguments.
fn joined_reduction_of(args: &[SqlExpr]) -> Result<Option<(&SqlExpr, &[SqlExpr])>, Refusal> {
    if args.len() == 2
        && let SqlExpr::ReduceCollection { reducer: AggFn::StringAgg, collection, extras, .. } = &args[0]
        && extras.len() == 1
        && literal_elements(collection)?.is_some()
    {
        return Ok(Some((collection, extras)));
    }
    Ok(None)
}

/// The literal element list under a collection (an element-text LIST_TRANSFORM unwrapped); none for a runtime one.
fn literal_elements(coll: &SqlExpr) -> Result<Option<&[SqlExpr]>, Refusal> {
    Ok(match coll {
        SqlExpr::ArrayLit { elements, .. } => Some(elements),
        SqlExpr::Call { r#fn: SqlFn::ListTransform, args, .. } => match arg(args, 0)? {
            SqlExpr::ArrayLit { elements, .. } => Some(elements),
            _ => None,
        },
        _ => None,
    })
}

/// An element's text coercion (`coalesce(cast(x as varchar), 'TDSNull')`) dropped: the raw element.
fn unwrap_element_text(e: &SqlExpr) -> &SqlExpr {
    let mut inner = e;
    if let SqlExpr::Call { r#fn: SqlFn::Coalesce, args, .. } = inner
        && args.len() == 2
        && matches!(&args[1], SqlExpr::StringLit { value, .. } if value == TDS_NULL_CELL)
    {
        inner = &args[0];
    }
    match inner {
        SqlExpr::Cast { value, .. } => value,
        other => other,
    }
}

/// The engine isolates a user TDS join's result: a bare star over a join with named outputs is wrapped in a select of
/// each output, quoted (a placeholder's column bare), from `(select * from (a join b)) as "tdswrap__"`.
fn wrap_tds_join_top(query: SqlQuery) -> Result<SqlQuery, Refusal> {
    let SqlQuery::Select(s) = &query else {
        return Ok(query);
    };
    let bare_star_over_join = s.projections.is_empty()
        && !s.distinct
        && matches!(s.from, SqlSource::Join { .. })
        && s.r#where.is_none()
        && s.group_by.is_empty()
        && s.having.is_none()
        && s.qualify.is_none()
        && s.order_by.is_empty()
        && s.limit.is_none()
        && s.offset.is_none()
        && !s.outputs.is_empty();
    if !bare_star_over_join {
        return Ok(query);
    }
    const WRAP: &str = "tdswrap__";
    // a placeholder's columns spell unquoted
    let mut ph_cols = HashSet::new();
    let mut srcs = VecDeque::from([&s.from]);
    while let Some(x) = srcs.pop_front() {
        if let SqlSource::VarSetPlaceholder { outputs, .. } = x {
            ph_cols.extend(outputs.iter().map(|o| o.name.as_str()));
        }
        if let SqlSource::Join { left, right, .. } = x {
            srcs.push_back(left);
            srcs.push_back(right);
        }
    }
    let cols = s
        .outputs
        .iter()
        .map(|o| {
            let name = if ph_cols.contains(o.name.as_str()) { o.name.clone() } else { format!("\"{}\"", o.name) };
            let column = lite_sql::Column {
                table: Some(WRAP.to_owned()),
                name,
                r#type: lite_sql::TypeFact::Unknown,
                origin: Some(lite_sql::Origin::Derived),
            };
            crate::build::projection(SqlExpr::Column(column), Some(o.name.clone()), Some(o.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let SqlQuery::Select(s) = query else {
        unreachable!("matched above");
    };
    let wrapped = crate::build::select_of(cols, crate::build::subselect(SqlQuery::Select(s), WRAP, None), Vec::new())?;
    Ok(SqlQuery::Select(Box::new(wrapped)))
}

// ==================================================================
// The alias plan (the engine's replaceAliasName, audit 19 F2)
// ==================================================================

impl AliasPlan {
    /// The engine groups aliases by lowercased table name and numbers them 0..n-1 within each group in encounter
    /// order; a root-position scope's leftmost source spells "root" but still consumes its group's next index; a
    /// joined derived table's interior continues the group numbering (`root_scope` false); union branches are each a
    /// root scope.
    fn plan_query(&mut self, q: &SqlQuery, groups: &mut HashMap<String, i32>, root_scope: bool) {
        match q {
            SqlQuery::Select(s) => {
                if !matches!(s.from, SqlSource::Dual) {
                    self.plan_source(&s.from, root_scope, groups);
                }
                // a correlated EXISTS joins the same group numbering
                if let Some(wh) = &s.r#where {
                    self.plan_expr_subqueries(wh, groups);
                }
            }
            SqlQuery::Union(u) => u.branches.iter().for_each(|b| self.plan_query(b, groups, true)),
            SqlQuery::With(w) => {
                w.ctes.iter().for_each(|c| self.plan_query(&c.query, groups, true));
                self.plan_query(&w.body, groups, true);
            }
        }
    }

    fn plan_expr_subqueries(&mut self, e: &SqlExpr, groups: &mut HashMap<String, i32>) {
        if let SqlExpr::Exists { subquery, .. } = e {
            self.plan_query(subquery, groups, false);
            return;
        }
        for c in crate::typing::children(e) {
            self.plan_expr_subqueries(&c, groups);
        }
    }

    fn plan_source(&mut self, src: &SqlSource, leftmost: bool, groups: &mut HashMap<String, i32>) {
        match src {
            SqlSource::Table { name, alias, .. } => {
                // the bare table name, never schema-qualified
                let group = name.rsplit('.').next().unwrap_or(name).to_lowercase();
                let renamed = if leftmost {
                    // every root of a group spends one index
                    self.consume_root_slot(&group, groups);
                    "root".to_owned()
                } else {
                    next_in_group(&group, groups)
                };
                self.renames.insert(alias.clone(), renamed);
            }
            SqlSource::Subselect { inner, alias, frame_name } => {
                // a named frame groups by its own name; any other by its first inner table
                let group = match frame_name {
                    Some(f) if f != EXISTS_KEYS_FRAME => f.to_lowercase(),
                    _ => first_inner_table(inner),
                };
                // only the DISTINCT materialization keeps the root name (its frame replaces the root table)
                let root_frame = leftmost && matches!(inner.as_ref(), SqlQuery::Select(ds) if ds.distinct);
                let renamed = if root_frame {
                    self.consume_root_slot(&group, groups);
                    "root".to_owned()
                } else {
                    // pre-order: the alias numbers before its interior
                    next_in_group(&group, groups)
                };
                self.renames.insert(alias.clone(), renamed);
                self.plan_query(inner, groups, frame_name.as_deref() != Some(EXISTS_KEYS_FRAME));
                self.subselects.insert(alias.clone(), frame_read(inner, frame_name.as_deref()));
            }
            SqlSource::Join { left, right, .. } => {
                self.plan_source(left, leftmost, groups);
                self.plan_source(right, false, groups);
            }
            // a cross-store plan variable groups by its variable name
            SqlSource::VarSetPlaceholder { var_name, alias, .. } => {
                let renamed = next_in_group(&var_name.to_lowercase(), groups);
                self.renames.insert(alias.clone(), renamed);
                self.placeholders.insert(alias.clone());
            }
            SqlSource::Cte { .. }
            | SqlSource::Pivot { .. }
            | SqlSource::Values { .. }
            | SqlSource::Dual
            | SqlSource::SourceUrl { .. }
            | SqlSource::RawSql { .. }
            | SqlSource::Catalog { .. } => {}
        }
    }

    /// One index per group for all its root-named members.
    fn consume_root_slot(&mut self, group: &str, groups: &mut HashMap<String, i32>) {
        if self.root_consumed.insert(group.to_owned()) {
            next_in_group(group, groups);
        }
    }
}

/// `group_<i>`, `i` the group's next index.
fn next_in_group(group: &str, groups: &mut HashMap<String, i32>) -> String {
    let n = groups.entry(group.to_owned()).or_insert(0);
    *n += 1;
    format!("{group}_{}", *n - 1)
}

/// The group a subselect alias renames into: its leftmost inner table's lowercased name.
fn first_inner_table(q: &SqlQuery) -> String {
    let mut src = match q {
        SqlQuery::Select(s) => Some(&s.from),
        SqlQuery::Union(u) => match u.branches.first() {
            Some(SqlQuery::Select(bs)) => Some(&bs.from),
            _ => None,
        },
        SqlQuery::With(_) => None,
    };
    while let Some(SqlSource::Join { left, .. }) = src {
        src = Some(left);
    }
    match src {
        Some(SqlSource::Cte { name, .. }) => name.to_lowercase(),
        Some(SqlSource::Table { name, .. }) => name.rsplit('.').next().unwrap_or(name).to_lowercase(),
        Some(SqlSource::Subselect { inner, .. }) => first_inner_table(inner),
        Some(SqlSource::VarSetPlaceholder { var_name, .. }) => var_name.to_lowercase(),
        _ => "subselect".to_owned(),
    }
}

/// What a column read of the frame reads: a union frame's outputs, a plain frame's projection aliases, else nothing
/// quoted.
fn frame_read(inner: &SqlQuery, frame_name: Option<&str>) -> FrameRead {
    if frame_name == Some(UNION_ALIAS) {
        return FrameRead::Union(ansi::query_outputs(inner).iter().map(|o| o.name.clone()).collect());
    }
    match inner {
        SqlQuery::Select(is) if frame_name.is_none() && !is.distinct => {
            FrameRead::Aliases(is.projections.iter().filter_map(|p| p.alias.clone()).collect())
        }
        _ => FrameRead::Physical,
    }
}

// ==================================================================
// The renderer
// ==================================================================

impl AnsiSqlRenderer for EngineStyle {
    fn lexicon(&self) -> &Lexicon {
        &LEXICON
    }

    fn ansi_state(&self) -> &AnsiState {
        &self.ansi
    }

    fn simple_name(&self) -> &'static str {
        self.profile.simple_name()
    }

    /// The query planned with the engine's aliases, written on one line.
    fn render(&self, query: SqlQuery) -> Result<String, Refusal> {
        let planned = self.planned(query, true)?;
        let mut w = SqlWriter::new();
        self.query(&mut w, &planned, 0)?;
        w.into_text()
    }

    /// The legacy plan text binds nothing: its parameters are the engine's template variables, written into the text.
    fn render_statement(&self, query: SqlQuery) -> Result<RenderedStatement, Refusal> {
        let _ = query;
        Err(Refusal::capability(
            "the legacy engine-text plan binds no parameters: its parameters are template variables in its text \
             (render)",
        ))
    }

    /// A seed row as the engine's H2 seed text writes it; every other statement the base's.
    fn render_dml(&self, dml: &SqlDml) -> Result<String, Refusal> {
        match dml {
            SqlDml::InsertValues { .. } => self.seed_row(dml),
            SqlDml::InsertFromTable { .. }
            | SqlDml::DeleteAll { .. }
            | SqlDml::CreateTempTableAs { .. }
            | SqlDml::InsertSelect { .. }
            | SqlDml::Update { .. }
            | SqlDml::DeleteWhere { .. }
            | SqlDml::Merge { .. } => ansi::render_dml(self, dml),
        }
    }

    /// The engine's H2 text is this text's own language.
    fn from_engine_text(&self, statement: &str) -> Result<String, Refusal> {
        Ok(statement.to_owned())
    }

    /// H2's carrier strategies (no literal reduction folded: this text spells the engine's chains), then the lateral
    /// explode as the engine's decorrelated union; no other pass.
    fn passes(&self) -> Vec<Box<dyn SqlRewriter>> {
        vec![Box::new(CarrierStrategies::with_literal_folds(CAPS, false)), Box::new(LateralExplodeToUnion)]
    }

    /// The engine's text reads every column as stored.
    fn reads_stored(t: &ColumnType) -> bool {
        let _ = t;
        false
    }

    /// The engine's WITH and UNION on one line, lowercase; a select its own.
    fn query(&self, w: &mut SqlWriter, q: &SqlQuery, depth: usize) -> Result<(), Refusal> {
        match q {
            SqlQuery::With(with) => {
                w.append("with ");
                for (i, c) in with.ctes.iter().enumerate() {
                    if i > 0 {
                        w.append(", ");
                    }
                    w.append(&c.name).append(" as (");
                    self.query(w, &c.query, depth)?;
                    w.push(')');
                }
                w.push(' ');
                self.query(w, &with.body, depth)
            }
            SqlQuery::Union(u) => {
                let op = if u.all { " union all " } else { " union " };
                for (i, b) in u.branches.iter().enumerate() {
                    if i > 0 {
                        w.append(op);
                    }
                    self.query(w, b, depth)?;
                }
                Ok(())
            }
            SqlQuery::Select(_) => ansi::query(self, w, q, depth),
        }
    }

    /// `select [top n] [distinct] ... from ... where ... group by ... having ... order by ... [offset .. fetch next]`,
    /// QUALIFY refused (the engine's text has none).
    fn select(&self, w: &mut SqlWriter, s: &SqlSelect, depth: usize) -> Result<(), Refusal> {
        if s.qualify.is_some() {
            return Err(Refusal::illegal_state("QUALIFY has no engine-H2 golden spelling"));
        }
        w.append("select ");
        self.select_head(w, s)?;
        if !matches!(s.from, SqlSource::Dual) {
            w.append(" from ");
            self.source(w, &s.from, depth)?;
        }
        if let Some(wh) = &s.r#where {
            w.append(" where ");
            self.where_sql(w, wh)?;
        }
        if !s.group_by.is_empty() {
            w.append(" group by ").join(&s.group_by, self.group_by_separator(), |w, e| self.group_key(w, s, e))?;
        }
        if let Some(h) = &s.having {
            w.append(" having ").expr(self, h, 0)?;
        }
        if !s.order_by.is_empty() {
            w.append(" order by ").join(&s.order_by, ", ", |w, k| self.sort_key(w, k))?;
        }
        if let Some(offset) = s.offset {
            w.append(" offset ").append_long(offset).append(" rows");
            if let Some(limit) = s.limit {
                w.append(" fetch next ").append_long(limit).append(" rows only");
            }
        }
        Ok(())
    }

    fn projection(&self, w: &mut SqlWriter, p: &Projection) -> Result<(), Refusal> {
        self.write_projection(w, &p.expr, p.alias.as_deref())
    }

    /// An output-column key spelled quoted, the direction explicit and lowercase; the null placement suppressed (the
    /// IR cannot tell the query's from Pure's own, PARK-18).
    fn sort_key(&self, w: &mut SqlWriter, k: &SortKey) -> Result<(), Refusal> {
        if let Some(name) = &k.output_name {
            w.append("\"").append(&name.replace('"', "")).append("\"");
        } else if let SqlExpr::Column(c) = &k.expr
            && c.table.is_none()
        {
            w.append("\"").append(&c.name).append("\"");
        } else {
            w.expr(self, &k.expr, 0)?;
        }
        w.append(if k.ascending { " asc" } else { " desc" });
        Ok(())
    }

    fn source(&self, w: &mut SqlWriter, src: &SqlSource, depth: usize) -> Result<(), Refusal> {
        match src {
            SqlSource::Cte { name, alias, .. } => {
                let a = self.alias_ident(alias);
                w.append(name).append(" as ").append(&a);
            }
            SqlSource::Table { name, alias, call, .. } => {
                if self.quote_identifiers {
                    let parts: Vec<String> = name.split('.').map(|x| format!("\"{x}\"")).collect();
                    w.append(&parts.join("."));
                } else {
                    w.append(name);
                }
                if *call {
                    w.append("()");
                }
                w.append(" as \"").append(&self.rename(alias)).append("\"");
            }
            SqlSource::Subselect { inner, alias, frame_name } => {
                w.append("(");
                // a union frame keeps its quoted interior; only a view frame unquotes its projections
                let f = frame_name.as_deref();
                let view_frame = f.is_some_and(|f| f != UNION_ALIAS && f != EXISTS_KEYS_FRAME);
                let anon_frame = f == Some(EXISTS_KEYS_FRAME);
                if view_frame {
                    self.frame_depth.set(self.frame_depth.get() + 1);
                }
                if anon_frame {
                    self.anon_distinct_depth.set(self.anon_distinct_depth.get() + 1);
                }
                let written = self.query(w, inner, depth);
                if view_frame {
                    self.frame_depth.set(self.frame_depth.get() - 1);
                }
                if anon_frame {
                    self.anon_distinct_depth.set(self.anon_distinct_depth.get() - 1);
                }
                written?;
                w.append(") as \"").append(&self.rename(alias)).append("\"");
            }
            SqlSource::VarSetPlaceholder { var_name, alias, .. } => {
                w.append("(${").append(var_name).append("}) as \"").append(&self.rename(alias)).append("\"");
            }
            SqlSource::Join { left, right, kind, on, .. } => {
                self.source(w, left, depth)?;
                let keyword = self.join_keyword(*kind)?;
                w.append(" ").append(&keyword).append(" ");
                self.source(w, right, depth)?;
                if let Some(on) = on {
                    w.append(" on (").expr(self, on, 0)?.append(")");
                }
            }
            SqlSource::Pivot { .. }
            | SqlSource::Values { .. }
            | SqlSource::Dual
            | SqlSource::SourceUrl { .. }
            | SqlSource::RawSql { .. }
            | SqlSource::Catalog { .. } => return ansi::source(self, w, src, depth),
        }
        Ok(())
    }

    /// legend-engine's join keywords, lowercase.
    fn join_keyword(&self, kind: JoinKind) -> Result<String, Refusal> {
        Ok(match kind {
            JoinKind::Inner => "inner join",
            JoinKind::Left => "left outer join",
            JoinKind::Right => "right outer join",
            JoinKind::Full => "full outer join",
            JoinKind::Cross => "cross join",
            JoinKind::CrossLateral => "cross join lateral",
            JoinKind::AsofLeft => "asof left join",
            JoinKind::LeftLateral => "left join lateral",
        }
        .to_owned())
    }

    /// Checked narrowing as the engine writes it: the inner value verbatim (toOne is processNoOp in its SQL).
    fn checked_one(
        &self,
        w: &mut SqlWriter,
        list: &SqlExpr,
        scalar_carrier: bool,
        at_least_only: bool,
        parent_prec: i32,
    ) -> Result<(), Refusal> {
        let _ = (scalar_carrier, at_least_only);
        w.expr(self, list, parent_prec)?;
        Ok(())
    }

    /// Carrier compaction as the engine writes it: the inner value verbatim.
    fn compact_list(&self, w: &mut SqlWriter, list: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        w.expr(self, list, parent_prec)?;
        Ok(())
    }

    /// DB2's null-safe OR expansions (`extensionDefaults.pure`), then the engine's H2 forms.
    fn expr(&self, w: &mut SqlWriter, e: &SqlExpr, parent_prec: i32) -> Result<(), Refusal> {
        if self.profile.db2()
            && let SqlExpr::Call { r#fn: f @ (SqlFn::NullSafeEqual | SqlFn::NullSafeNotEqual), args, .. } = e
        {
            let a = |w: &mut SqlWriter| w.expr(self, arg(args, 0)?, 4).map(drop);
            let b = |w: &mut SqlWriter| w.expr(self, arg(args, 1)?, 4).map(drop);
            if *f == SqlFn::NullSafeEqual {
                w.append("(").piece(&a)?.append(" = ").piece(&b)?.append(" or (").piece(&a)?.append(" is null and ");
                w.piece(&b)?.append(" is null))");
            } else {
                w.append("(not (").piece(&a)?.append(" = ").piece(&b)?.append(") or (").piece(&a)?;
                w.append(" is null and ").piece(&b)?.append(" is not null) or (").piece(&a)?;
                w.append(" is not null and ").piece(&b)?.append(" is null))");
            }
            return Ok(());
        }
        self.h2_expr(w, e, parent_prec)
    }

    /// Expression membership: `x in (<collection>)`, a literal collection's elements spelled.
    fn membership(&self, w: &mut SqlWriter, needle: &SqlExpr, collection: &SqlExpr) -> Result<(), Refusal> {
        let mut coll = collection;
        while let SqlExpr::Cast { value, target: SqlType::Array { .. }, .. } = coll {
            coll = value;
        }
        w.expr(self, needle, 4)?.append(" in (");
        match literal_elements(coll)? {
            Some(elements) => w.list(self, elements)?,
            None => w.expr(self, coll, 0)?,
        };
        w.append(")");
        Ok(())
    }

    fn call(&self, w: &mut SqlWriter, f: SqlFn, a: &[SqlExpr], parent_prec: i32) -> Result<(), Refusal> {
        match self.profile {
            Profile::H2 => self.h2_call(w, f, a, parent_prec),
            Profile::Db2 => self.db2_call(w, f, a, parent_prec),
            Profile::Composite => self.composite_call(w, f, a, parent_prec),
        }
    }

    /// JSON_TYPE answers DuckDB's names (the engine text's json_type, read on DuckDB).
    fn json_kind_name(&self, kind: JsonKind) -> Result<String, Refusal> {
        Ok(match kind {
            JsonKind::Object => "OBJECT",
            JsonKind::Array => "ARRAY",
            JsonKind::Null => "NULL",
            JsonKind::String => "VARCHAR",
            JsonKind::Integer => "BIGINT",
            JsonKind::UnsignedInteger => "UBIGINT",
            JsonKind::Float => "DOUBLE",
            JsonKind::Boolean => "BOOLEAN",
        }
        .to_owned())
    }

    /// A DATE is `DATE`; no other type's name is compared.
    fn typeof_name(&self, scalar: Scalar) -> Result<String, Refusal> {
        if scalar == Scalar::Date {
            return Ok("DATE".to_owned());
        }
        Err(Refusal::capability(format!("a TYPEOF compared with {scalar}'s name reached the engine text")))
    }

    /// Pure's round: the engine's bare `round(x[, n])`.
    fn round_half_even(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("round(").expr(self, arg(a, 0)?, 0)?;
        if a.len() > 1 {
            w.append(", ").expr(self, &a[1], 0)?;
        }
        w.append(")");
        Ok(())
    }

    /// The engine's list spellings: greatest/least over a literal list (empty: over null) and the mixed-identity
    /// selection over its comparables, a reduction over a literal list as its infix chain, firstNotNull as coalesce.
    fn list_call(&self, w: &mut SqlWriter, f: SqlFn, args: &[SqlExpr]) -> Result<(), Refusal> {
        let extreme = matches!(f, SqlFn::ListMax | SqlFn::ListMin);
        if extreme && let SqlExpr::ArrayLit { elements, .. } = arg(args, 0)? {
            return self.variadic_extreme(w, f == SqlFn::ListMax, elements);
        }
        if extreme && empty_collection(arg(args, 0)?) {
            return self.variadic_extreme(w, f == SqlFn::ListMax, &[]);
        }
        // ids[list_position(vals, list_max(vals))]: the same variadic form over the comparables
        if f == SqlFn::ListGet
            && args.len() == 2
            && matches!(args[0], SqlExpr::ArrayLit { .. })
            && let SqlExpr::Call { r#fn: SqlFn::ListPosition, args: pos, .. } = &args[1]
            && pos.len() == 2
            && let SqlExpr::ArrayLit { elements: vals, .. } = &pos[0]
            && let SqlExpr::Call { r#fn: win @ (SqlFn::ListMax | SqlFn::ListMin), .. } = &pos[1]
        {
            return self.variadic_extreme(w, *win == SqlFn::ListMax, vals);
        }
        // a reduction over a literal list: the engine's infix chain
        if args.len() == 1
            && let SqlExpr::ArrayLit { elements: lit, .. } = &args[0]
            && !lit.is_empty()
        {
            let op = match f {
                SqlFn::ListBoolAnd => Some(SqlFn::And),
                SqlFn::ListBoolOr => Some(SqlFn::Or),
                SqlFn::ListProduct => Some(SqlFn::Times),
                SqlFn::ListSum => Some(SqlFn::Plus),
                _ => None,
            };
            if let Some(op) = op {
                let elems: Vec<SqlExpr> = lit.iter().map(|e| uncast_literal(e).clone()).collect();
                if elems.len() == 1 {
                    w.expr(self, &elems[0], 0)?;
                    return Ok(());
                }
                let chain = crate::build::call(op, elems);
                // boolean chains are flat; arithmetic ones parenthesize
                let prec = if matches!(op, SqlFn::And | SqlFn::Or) { 0 } else { i32::MAX };
                w.expr(self, &chain, prec)?;
                return Ok(());
            }
        }
        // firstNotNull over a literal list: coalesce(a, b, ...)
        if f == SqlFn::ListGet
            && args.len() == 2
            && matches!(args[1], SqlExpr::IntLit { value: 1, .. })
            && let SqlExpr::Call { r#fn: SqlFn::ListFilter, args: flt, .. } = &args[0]
            && flt.len() == 2
            && let SqlExpr::ArrayLit { elements: src, .. } = &flt[0]
            && !src.is_empty()
            && let SqlExpr::Lambda(pred) = &flt[1]
            && pred.params.len() == 1
            && is_not_null_of(&pred.body, &pred.params[0])
        {
            w.expr(self, &crate::build::call(SqlFn::Coalesce, src.clone()), 0)?;
            return Ok(());
        }
        Err(Refusal::capability(format!("{f} reached a dialect without a list encoding")))
    }

    /// splitPart: the engine's own H2 extension function.
    fn split_part_call(&self, w: &mut SqlWriter, a: &[SqlExpr]) -> Result<(), Refusal> {
        w.append("legend_h2_extension_split_part(").expr(self, arg(a, 0)?, 0)?.append(", ");
        w.expr(self, arg(a, 1)?, 0)?.append(", ").expr(self, arg(a, 2)?, 0)?.append(")");
        Ok(())
    }

    /// A conformance or temporal-text cast elided; `decimal`, `float` for the DECIMAL(38, 18) and double casts; DB2's
    /// strings `varchar(16000)` and its date-typed parses bare.
    fn variant_aware_cast(
        &self,
        w: &mut SqlWriter,
        value: &SqlExpr,
        target: &SqlType,
        conform: bool,
    ) -> Result<(), Refusal> {
        if self.profile.db2() {
            if conform {
                w.expr(self, value, 0)?;
                return Ok(());
            }
            if *target == SqlType::Scalar(Scalar::Varchar) {
                w.append("cast(").expr(self, value, 0)?.append(" as varchar(16000))");
                return Ok(());
            }
            if matches!(target, SqlType::Scalar(Scalar::Date | Scalar::Timestamp))
                && matches!(value, SqlExpr::Call { r#fn: SqlFn::Strptime | SqlFn::AddInterval, .. })
            {
                w.expr(self, value, 0)?;
                return Ok(());
            }
        }
        if conform || *target == SqlType::Scalar(Scalar::TemporalText) {
            w.expr(self, value, 0)?;
            return Ok(());
        }
        let mut t = ansi::cast_type_name(self, target)?.to_lowercase();
        if t == "decimal(38, 18)" {
            t = "decimal".to_owned();
        } else if t == "double precision" || t == "double" {
            t = "float".to_owned();
        }
        w.append("cast(").expr(self, value, 0)?.append(" as ").append(&t).append(")");
        Ok(())
    }

    /// An ordered string aggregate over a window: its WITHIN GROUP after the window.
    fn window_call(
        &self,
        w: &mut SqlWriter,
        f: &SqlAgg,
        partition_by: &[lite_sql::SqlExpr],
        order_by: &[SortKey],
        frame: Option<&Frame>,
    ) -> Result<(), Refusal> {
        if let SqlAgg::Reducer(r) = f
            && r.r#fn == AggFn::StringAgg
            && !r.order_by.is_empty()
        {
            self.listagg(w, r)?;
            ansi::over(self, w, partition_by, order_by, frame)?;
            return self.within_group(w, r);
        }
        ansi::window_call(self, w, f, partition_by, order_by, frame)
    }

    /// legend-engine's H2 keywords: lowercase.
    fn keyword(&self, keyword: &str) -> String {
        keyword.to_lowercase()
    }

    /// legend-engine's H2 window function names: lowercase.
    fn aggregate_name(&self, f: AggFn) -> String {
        f.as_str().to_lowercase()
    }

    /// The engine's aggregate names, lowercase; the string aggregate `listagg`, the percentiles within group.
    fn reducer(&self, w: &mut SqlWriter, r: &Reducer) -> Result<(), Refusal> {
        let agg = |w: &mut SqlWriter, name: &str| ansi::aggregate(self, w, name, r);
        match r.r#fn {
            AggFn::Sum => agg(w, "sum"),
            AggFn::Count => agg(w, "count"),
            AggFn::Avg => agg(w, "avg"),
            AggFn::Min => agg(w, "min"),
            AggFn::Max => agg(w, "max"),
            // the H2-lenient per-group witness: the bare expression
            AggFn::AnyValue => {
                if r.args.len() == 1 {
                    w.expr(self, &r.args[0], 0)?;
                    Ok(())
                } else {
                    agg(w, "any_value")
                }
            }
            AggFn::StddevSamp => agg(w, "stddev_samp"),
            AggFn::StddevPop => agg(w, "stddev_pop"),
            AggFn::VarSamp => agg(w, "var_samp"),
            AggFn::VarPop => agg(w, "var_pop"),
            AggFn::Median => agg(w, "median"),
            AggFn::Mode => agg(w, "mode"),
            // an ordered string aggregate: listagg ... within group
            AggFn::StringAgg => {
                if r.order_by.is_empty() {
                    agg(w, "listagg")
                } else {
                    self.listagg(w, r)?;
                    self.within_group(w, r)
                }
            }
            AggFn::List => agg(w, "list"),
            AggFn::QuantileCont => {
                if within_group_shaped(r) {
                    self.percentile(w, "percentile_cont", r)
                } else {
                    agg(w, "quantile_cont")
                }
            }
            AggFn::QuantileDisc => {
                if within_group_shaped(r) {
                    self.percentile(w, "percentile_disc", r)
                } else {
                    agg(w, "quantile_disc")
                }
            }
            AggFn::Corr => agg(w, "corr"),
            AggFn::CovarSamp => agg(w, "covar_samp"),
            AggFn::CovarPop => agg(w, "covar_pop"),
            AggFn::ArgMax => agg(w, "arg_max"),
            AggFn::ArgMin => agg(w, "arg_min"),
            AggFn::BoolAnd => agg(w, "bool_and"),
            AggFn::BoolOr => agg(w, "bool_or"),
            AggFn::Variance => agg(w, "variance"),
            AggFn::Stddev => agg(w, "stddev"),
            AggFn::Wavg | AggFn::HashList | AggFn::IsDistinctMark | AggFn::UniqueValueOnly => {
                Err(ansi::lowering_marker(r))
            }
            AggFn::RowNumber
            | AggFn::Rank
            | AggFn::DenseRank
            | AggFn::PercentRank
            | AggFn::CumeDist
            | AggFn::Ntile
            | AggFn::Lag
            | AggFn::Lead
            | AggFn::FirstValue
            | AggFn::LastValue
            | AggFn::NthValue => Err(ansi::window_function(r)),
        }
    }

    /// The NULLS placement inside an ordered aggregate suppressed, as the sort key's.
    fn agg_order_null_placement(&self, k: &SortKey) -> String {
        let _ = k;
        String::new()
    }

    /// The engine's H2-new date spelling, no space: `DATE'2005-10-10'`.
    fn date_lit(&self, iso: &str) -> String {
        format!("DATE'{iso}'")
    }

    /// `TIMESTAMP'...'`, its date-time separator a space; under a connection's zone other than GMT, the constant
    /// shifted from GMT to that zone (engine adjustDate).
    fn timestamp_lit(&self, iso: &str) -> Result<String, Refusal> {
        let mut iso = iso.to_owned();
        if let Some(at) = byte_of_unit(&iso, 10)
            && iso[at..].starts_with('T')
        {
            iso.replace_range(at..at + 1, " ");
        }
        if let Some(tz) = &self.time_zone
            && tz != "GMT"
            && utf16_len(&iso) >= 19
            && let Some(shifted) = java_time::shift_to_zone(&iso, tz)?
        {
            iso = shifted;
        }
        Ok(format!("TIMESTAMP'{iso}'"))
    }

    fn time_lit(&self, iso: &str) -> Result<String, Refusal> {
        let _ = iso;
        Err(Refusal::capability(
            "a time of day: legend-engine's relational lowering refuses StrictTime, so its text has no spelling for one",
        ))
    }

    /// Every scalar type's name, a refusal where it has none.
    fn scalar_type_name(&self, s: Scalar) -> Result<String, Refusal> {
        Ok(match s {
            Scalar::Boolean => "BOOLEAN",
            Scalar::Integer => "INTEGER",
            Scalar::Bigint => "BIGINT",
            Scalar::Hugeint => "HUGEINT",
            Scalar::Double => "DOUBLE PRECISION",
            Scalar::Varchar | Scalar::TemporalText => "VARCHAR",
            Scalar::Date => "DATE",
            Scalar::Timestamp => "TIMESTAMP",
            Scalar::Timestamptz => "TIMESTAMPTZ",
            // legend-engine's relational lowering refuses StrictTime
            Scalar::Time => {
                return Err(Refusal::capability(
                    "a time of day: legend-engine's relational lowering refuses StrictTime, so its text has no \
                     spelling for one",
                ));
            }
            Scalar::Json | Scalar::DecimalText => {
                return Err(Refusal::illegal_state(format!("{s} cast reached a dialect without {s} support")));
            }
        }
        .to_owned())
    }

    /// No STRUCT type here: a struct cast is refused.
    fn struct_type_name(&self, fields: &[StructField]) -> Result<String, Refusal> {
        let _ = fields;
        Err(Refusal::capability("a STRUCT type reached a dialect without struct support"))
    }

    /// A date format's parts: the base's strftime codes.
    fn format_text(&self, parts: &[DateFmt]) -> Result<String, Refusal> {
        Ok(ansi::strftime_text(parts))
    }

    /// DB2's dynafunction templates print `-` and `+` tight (`position(...)-1`).
    fn infix_pad(&self, f: SqlFn) -> String {
        if self.profile.db2() && matches!(f, SqlFn::Minus | SqlFn::Plus) { "" } else { " " }.to_owned()
    }

    /// A stored table's and schema's names as the store wrote them, declared quoted or not.
    fn stored_name(&self, name: &str, declared_quoted: bool) -> String {
        let _ = declared_quoted;
        AnsiSqlRenderer::physical_name(self, name)
    }

    /// The engine's `dataTypeToSqlText`.
    fn ddl_type(&self, t: &ColumnType) -> Result<String, Refusal> {
        ddl_engine_text(t)
    }

    /// The engine's processColumnName: a declared-quoted name quoted, any other by the engine's rule.
    fn ddl_identifier(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { format!("\"{name}\"") } else { engine_column_name(name) }
    }

    /// The engine's rule plus the execution necessity: a name that is not a plain identifier quoted (its quotes
    /// dropped).
    fn insert_column_name(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted {
            return format!("\"{name}\"");
        }
        let mut chars = name.chars();
        let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain {
            return format!("\"{}\"", name.replace('"', ""));
        }
        engine_column_name(name)
    }

    /// The metamodel names joined raw in `PRIMARY KEY(...)`: a declared-quoted name carries its quotes.
    fn ddl_key_identifier(&self, name: &str, declared_quoted: bool) -> String {
        if declared_quoted { format!("\"{name}\"") } else { name.to_owned() }
    }

    fn ddl_column_separator(&self) -> String {
        ",".to_owned()
    }
}

/// A percentile the engine writes within group: a value and a level, at most one order key.
fn within_group_shaped(r: &Reducer) -> bool {
    r.args.len() == 2 && !r.distinct && r.order_by.len() <= 1
}

/// The byte where UTF-16 unit `unit` of `s` starts, when a character starts there.
fn byte_of_unit(s: &str, unit: usize) -> Option<usize> {
    let mut units = 0;
    for (at, c) in s.char_indices() {
        if units == unit {
            return Some(at);
        }
        if units > unit {
            return None;
        }
        units += c.len_utf16();
    }
    None
}

// ==================================================================
// The engine-text surface and the SqlDialect
// ==================================================================

impl EngineTextDialect for EngineStyle {
    /// The engine's `renderCollection` template: the separator `,` and the kind's prefix, suffix and escapes (each
    /// escape pair with a trailing space).
    fn collection_splice(&self, cp: &PlanParam<'_>) -> Result<String, Refusal> {
        Ok(format!(
            "${{renderCollection({}![] \",\" {} \"null\")}}",
            cp.name,
            self.holder_args(cp.kind)?.replace("\"}", "\" }")
        ))
    }

    fn rendered_alias(&self, alias: &str) -> String {
        self.rename(alias)
    }

    fn select_list(&self, query: SqlQuery) -> Result<Option<String>, Refusal> {
        let mut q = &self.planned(query, true)?;
        while let SqlQuery::Union(u) = q {
            q = arg_query(&u.branches)?;
        }
        let SqlQuery::Select(s) = q else {
            return Ok(None);
        };
        let mut w = SqlWriter::new();
        self.select_head(&mut w, s)?;
        w.into_text().map(Some)
    }

    fn render_as_aliased(&self, query: SqlQuery) -> Result<String, Refusal> {
        let planned = self.planned(query, false)?;
        let mut w = SqlWriter::new();
        self.query(&mut w, &planned, 0)?;
        w.into_text()
    }

    /// The engine aliases the table by its name, lower case, and `_0`, quoted.
    fn in_temp_table_select(&self, temp_table: &str) -> String {
        let alias = format!("\"{}_0\"", temp_table.to_lowercase());
        format!(
            "select {alias}.ColumnForStoringInCollection as ColumnForStoringInCollection from {temp_table} as {alias}"
        )
    }

    fn plan_aliases(&self, query: SqlQuery) -> Result<(), Refusal> {
        self.planned(query, true).map(drop)
    }
}

/// `u.branches().get(0)`.
fn arg_query(branches: &[SqlQuery]) -> Result<&SqlQuery, Refusal> {
    branches.first().ok_or_else(|| Refusal {
        class: Refusal::INDEX_OUT_OF_BOUNDS,
        message: format!("Index 0 out of bounds for length {}", branches.len()),
    })
}

impl SqlDialect for EngineStyle {
    fn dialect_name(&self) -> &'static str {
        self.profile.simple_name()
    }

    fn engine_text(&self) -> Option<&dyn EngineTextDialect> {
        Some(self)
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
}

#[cfg(test)]
mod tests;
