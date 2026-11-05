# DataCube: architecture

A newcomer's map of DataCube: what it is, its pieces, where its state lives, how a cube becomes a query, and the
decisions from its first designs that still hold. Each piece's own design document is linked where the piece is
described, and all of them are listed in §7. Code paths are relative to `datacube/src/` unless they say otherwise; they
name today's TypeScript, which the Rust port keeps through its first iteration.

## 1. What DataCube is

DataCube is legend-lite's tool for looking at data: a grid that filters, groups, pivots and adds calculated columns
over any relation; charts, KPIs, text and timeseries tiles beside the grids; pages of several sheets that are saved,
shared and opened again; and, as a mode of the same pages, a visual ETL tool (flow mode). It is written in TypeScript
and runs in three places: a browser tab, a Python host's tab or notebook widget, and headless inside Python with no
screen at all.

Three rules shape all of it:

- **DataCube computes nothing.** Every figure it shows is a Pure query that legend-lite's compiler plans to SQL and a
  database runs. The browser draws rows it is handed; it never aggregates, joins or windows them.
- **DataCube's logic is DataCube's TypeScript, wherever it runs.** It is never copied into Python and never ported to
  Java; Python runs the same code in an embedded V8 and asks it (§2.7).
- **Every change is a command.** A change to a grid is a `GridCommand`, a change to a page a `PageCommand`, each applied
  by one function (`grid-commands.ts` `applyGridCommand`, `page-commands.ts` `applyPageCommand`), whether the screen or
  Python made it. So undo, save, share and Python come with every feature
  (`docs/DATACUBE_ONE_API_PLAN_2026_10_21.md`).

## 2. The pieces

| Piece | What it is | Today's code | Its design |
|---|---|---|---|
| The grid | One cube: a source, its query settings, its view | `app.ts` (`CubeApp`), `cube.ts` (`CubeController`), `cube-state.ts`, `query.ts` | §5 below; `docs/DATACUBE_LIVE_EDITING_REDESIGN_2026_10_24.md` |
| Pages | Sheets of tiles (grids, charts, text, timeseries), laid out in bands, saved whole | `page/page-app.ts`, `page/cube-page.ts`, `page-document.ts`, `layout/bands.ts` | `docs/DATACUBE_PAGES_DESIGN_2026_10_09.md` |
| Charts | A view of one grid's query, regrouped; drawn by ECharts | `chart-spec.ts`, `chart-render.ts` | `docs/DATACUBE_CHARTS_DESIGN_2026_10_09.md`; §5.5 |
| Prep and the model | Shape data, make classes from it, build dashboards on classes | `create-class.ts`, `model-actions.ts`, `join.ts`, `class-source.ts`, `page-filters.ts` | `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md` |
| Flow mode | A sheet drawn as a pipeline of boxes, each box a grid | `layout/flow.ts`, `layout/flow-board.ts`, `layout/flow-canvas.ts`, `flow-boxes.ts`, `live-inputs.ts` | `docs/DATACUBE_FLOW_MODE_2026_10_24.md` |
| Timeseries | A tile of many series from many grids, every figure the database's | `timeseries-spec.ts`, `timeseries-query.ts`, `pure-library.ts`, `ui/timeseries-view.ts` | `docs/DATACUBE_TIMESERIES_DESIGN_2026_10_25.md` |
| The app and its hosts | One app every host opens over its own services | `app/`, `services/browser.ts`, `services/python-host.ts` | `docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md` |
| Headless Python | DataCube's non-screen code in V8 inside Python | `headless/`, `python/legend_lite/_headless.py`, `page.py` | `docs/DATACUBE_HEADLESS_PYTHON_DESIGN_2026_10_11.md` |
| Planner, database, runner | Who writes the SQL, where the data is, who runs it | `lite-planner.ts`, `runner.ts`, `../../engine-client/src/` | `docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md` §4, §15 |

### 2.1 The grid

A grid is one cube: a source, the query settings a person makes on it (calculated columns, a filter, row groups, a
pivot, measures, sorts) and the view the database answers with. A grid is its **base query** and the cube's operations
on top of it: a single table is the shorthand `#>{db::DB.t}#`, a join or another grid's rows are base queries too, and
every question about a grid -- its type, what it reads (its leaves), where its rows are, what a class of it is -- is
asked of that query, never of its source's kind (model design §18). The source kinds are a file, a saved query, a
warehouse table, a remote file, a Python frame, a class, a join, and another grid's rows (`cube-document.ts`
`CubeSource`). Ad Hoc Analysis (`adhoc/`) is a second way of drawing one grid's state, over the same owner (§3).

### 2.2 Pages

A page owns its tiles; no grid is special. A page is one or more **sheets**; a sheet is a stack of **bands**, each a
small tree of rows and columns whose leaves are tiles, or a **stack** of tiles shown one at a time. A band layout scrolls
or fits the window. A page has a bar of its own (its menu, its name, the sheet tabs); each grid has its own header and
menu. What is looked at (the sheet shown, a stack's front tile) is not saved; what is on the page is. The page document
(`page-document.ts`, kind `datacube.page`) holds its cubes, its views (grid, chart, text and timeseries tiles), its
sheets with their layouts, and, where used, the page's model, page filters, parameters, macros and snapped copies.

### 2.3 Charts

A chart is a view of one grid: its query is the grid's own snapshot -- the same source, filter and calculated columns --
regrouped by the chart's encoding (x and a split become row groups, each y a measure), built by the same `levelLambda`
as the grid's levels. The database aggregates; ECharts only draws the rows it is handed (`chart-spec.ts`'s header). §5.5
has the lasting decisions.

### 2.4 Prep, the model, and dashboards on it

DataCube has two modes (model design §0). **Prep** shapes raw sources as relations -- filter, calculated columns, every
join legend-lite's relation API has, append, group -- and **Create a Class…** turns a shape into the model: the
pipeline as a relation function, a class of its columns, and a mapping of the class to the function (upstream's
`promoteRelationToModel`). Classes are related by Associations mapped by ModelJoins. **Dashboards** are pages over
classes; a page filter or a chart's click reaches other tiles by a class's property, through the relationships, and
nothing reaches across tiles without a model. A page filter reaches a grid as a lens: every query the grid runs is
rewritten with the class's filter first, and the grid's own state, undo and saved cube never hold it (`runner.ts`
`QueryLens`). The classes live in a project of the model home (§3).

### 2.5 Flow mode

A flow is a sheet of the page (layout kind `flow`), not a second app. A box is a grid that does one thing (a Filter box
is a grid with only a filter), boxes are linked by **live inputs** (a grid reading another grid's output, or one of its
ports, as it is now), and every gesture on the canvas is a page or grid command. A box is a document until it is shown:
only the box shown runs a query. Alteryx's tools are mapped one by one onto DataCube's own editors and presets; writes
(an Output into a table) are execution plans the planner makes and one TypeScript plan runner runs.

### 2.6 Timeseries

A timeseries tile is a fourth view kind beside grid, chart and text. It reads any number of grids; each series is one
query run where its grid runs, a series combining grids is still one query, and grids on other stores are snapped into
the tab's DuckDB first. The tile's functions (gs-quant's vocabulary) are DataCube's Pure library (`pure-library.ts`,
`datacube::ts`), functions generic over any table that the tile's queries call by name; the tile's TypeScript only
shapes (periods, lines, the window shown, alignment).

### 2.7 The app, its hosts, and headless Python

One app (`app/`, entered by `openDataCube(host, services)`) is opened by every host with a screen. A host supplies only
**services**: where queries are planned and run and where data goes in (engine), where pages are saved (pages), where
classes go (model home), where saved queries are (queries), and what to open first (start). What differs by host is a
capability its services offer, never a second path through the app.

Headless, Python runs DataCube's non-screen bundle in V8 (`mini-racer`) on its own thread. The bundle's planner is the
same `LitePlanner` over a worker transport whose "worker" is the host, answered by legend-lite's native library (the
same Java, built by GraalVM); its database is DataCube's `DuckDbEngine` over Python's DuckDB. Python's API is synchronous
and safe in Jupyter (one event loop on a background thread). Python asks DataCube every rule it needs and holds none of
its own: one rulebook, never a copy (`docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md` §16). When Python serves a page, its
keeper holds the page (§3).

### 2.8 Planner, database and runner: what DataCube talks to

A grid's rows have three parts (one-app design §15.2):

| | Chosen by | Options |
|---|---|---|
| **planner** (who writes the SQL) | the person, once per app | local -- the tab's WebAssembly module or Python's native library --, legend-lite's server, legend-engine |
| **connection** (where the data is) | the source picked | a file, an example or a remote file: the host's DuckDB; a warehouse table: that database |
| **runner** (who executes the SQL) | the host and the connection | DuckDB-WASM: the page; Python's DuckDB: Python; a warehouse: its own server |

The warehouse is legend-lite's warehouse server (`warehouse/`, Java today): SQL runs there as the signed-in user, under
the database's own grants, and comes back as Arrow; Snap copies what the user may read into the host's DuckDB
(`engine-client/src/warehouse.ts`). `docs/DATACUBE_ON_POSTGRES.md` is the user guide for DataCube over a Postgres
database. In the browser the planner module is frozen at the last TeaVM build (`wasm/planner/`, decided 2026-11-03)
until the Rust wasm32 build replaces it (`docs/rust-port/PHASE1.md` §5).

## 3. Where state lives

| State | Its one holder | How it changes |
|---|---|---|
| A grid's query settings and view | Its state owner (`cube-state.ts` `CubeStateOwner` over `state-owner.ts`): one immutable state, committed, rendered and pending (§5.2) | Grid commands; a change whose query differs is one transaction |
| A page: its cubes, tiles, sheets, layouts, filters, parameters, macros | The page document, in the page's state | Page commands, applied by one function; each a step of the page's history |
| A page Python serves | Python's keeper (`python/legend_lite/keeper.py`): the one copy and its version | Each editor writes the version it was at and the commands it made there; the keeper has DataCube apply them to the page as it is now (`pageRebase`) and keeps what they make, or refuses them whole |
| A snapped copy of a grid's rows | The page (`snap` and `live` commands, `copies` in the document); the copy itself in the host's database | Made by the host, let go when nothing reads it; a saved page opens live |
| Saved pages | The page service's store: upstream's DataCube query API (`CubeStore`), in IndexedDB in the tab, a folder on Python | Save, Save As…, Open… |
| Classes, associations, data spaces | A project in the model home, an SDLC: the in-page SDLC (IndexedDB) or a server in the tab; Python's SDLC over a git folder | Model actions, which are not undo steps; the page points to the project, pinned to a revision |
| The sheet shown, a stack's front tile, a flow's box shown, a selection | The screen | Never saved, never a page change |

A page never holds a copy of its model: it points to it. A page that travels (a draft shared, a page file, a notebook)
carries a bundle -- the page and its project's files at the pinned revision -- which the receiver imports into its own
model home.

## 4. How a cube becomes a query

1. **Types first.** Each refresh begins with one compile-only request for the relation type of
   `source->extend(calculated columns)` (`plan.ts` `typeColumns`; upstream's `lambdaRelationType`, cached by the
   planners by grammar). The snapshot adopts the compiler's types; a declared type the compiler no longer gives is a
   schema change, reported, never kept silently.
2. **The snapshot becomes protocol JSON.** `query.ts` builds every query DataCube sends as a lambda tree with
   pure-protocol's Relation API: one query per open level of the tree (`levelLambda`), and no literal spelled as text.
   Pure text exists only where a person reads or writes it, and the compiler owns both edges (it parses what was typed,
   and prints a query to read).
3. **A pivot is two plain queries**: one finding its values, then one `groupBy` per level with a conditional aggregate
   per value (§5.3). A chart is the grid's snapshot regrouped by its encoding. A page filter is a lens on every query
   the grid runs. A timeseries series is a query calling the Pure library.
4. **Planned over a held model.** Each host holds its compiled models (core's `HeldModels` over `HeldStore`, keyed by
   the model's content; the tab's in its WebAssembly module, Python's in its native library; a server holds none), so a
   plan costs the query alone, not the model's compile (model design §9).
5. **Plan and run, one path.** `PlanAndRun` (`runner.ts`) is the chosen planner's plan, the connection's runner
   executing that SQL, and the page reading the rows typed by the plan (`typedByPlan`). In the tab it plans and runs
   in-page (`PlanThenRun`); on a Python host the page sends Pure, never SQL, and Python plans and runs it in one call
   (`plan-and-run.json`); a remote server plans and runs with legend-engine's standard calls. A question about the Pure
   alone (a parse, a print, a query's columns) is legend-engine's own `pure/v1` request on every host. The planner is
   chosen once, always shown, and never substituted: one that does not answer is said, and the page stops.
6. **Values stay exact.** A cell keeps the database's exact form, as text where JavaScript has no type for it: a
   StrictDate `YYYY-MM-DD`, a DateTime as stored (UTC) to the microsecond, a Decimal's exact digits, an Integer a number
   when exact and a bigint beyond 2^53 (`engine-client/src/values.ts`). What a value means is its column's compiler
   type.

## 5. Decisions from the first designs that still hold

The six designs of 2026-09-27 and 2026-09-28 set rules the later designs build on. Each is stated here as the code has
it today; where a later design or the code changed a rule, the newer one is kept and the change is said.

### 5.1 Types come from the compiler

Every type fact the front end once decided for itself is owned by the compiler or generated from it:

| The fact | Its owner | How DataCube gets it |
|---|---|---|
| Every column's type, of every query | The compiler | `lambdaRelationType` for a query not being run; the plan's own result type for one being run (§4) |
| A type's family (numeric, temporal, text, variant, ...) | legend-lite's type lattice | Generated at build time by running it (`TypeFacts.java` writes `engine-client/src/generated/lite-facts.ts` and Python's `_type_facts.py`; decision D1) |
| Which aggregates and filter operators a column's type takes, and what they return | The compiler | Generated: DataCube's own queries over a column of each type, compiled by legend-lite (`datacube/tools/offer-facts`, `generated/offer-facts.ts`, diff-tested; D4) |
| A Pure literal in a query | The protocol | Queries built as protocol JSON, as upstream's DataCube builds them; a literal a typed node (D2) |
| A column's default kind and aggregate | A product rule, once | Every numeric column is a measure unless declared a dimension (D3): `query.ts` `defaultAggregate`, `sum` for a numeric column not declared a dimension, `unique` for any other |
| Number and date formatting | The client: presentation | Keyed on the compiler's type and scale, never on a value's own JavaScript type |

A DateTime is what Pure says it is: a relational TIMESTAMP read as UTC, shown as stored and written back at full
precision. Types the relational DDL cannot say are converted in the database, at the source, by what legend-lite's
writer reads from the catalog (`upload.ts` `modelLanded`): a TIMESTAMPTZ its UTC timestamp, a UBIGINT an exact
DECIMAL(20,0), a UUID or TIME its text; a nested STRUCT or LIST needs no conversion and is a Variant as stored. (The
first design converted nested types with `to_json`; the code no longer does.) Nothing is silently narrowed.

Not as decided: decision D5 put a JSON column's shape with the database, over the whole column. Today's
`json-shape.ts` still infers the shape in the client, from cells read through the ordinary query path -- a sample, or
every row streamed -- and uses it only to choose which `to(@T)` a generated column writes; the compiler then types that
column, so a wrong guess is a refusal or a visibly wrong column, never a silent one.

Sources: `docs/DATACUBE_TYPES_TO_SERVER_2026_09_27.md`, `docs/DATACUBE_TYPED_VALUES_DESIGN_2026_09_27.md`,
`docs/PYTHON_ASKS_DATACUBE_2026_10_15.md` (git history: docs/HISTORY.md).

### 5.2 One state owner

A grid's state has one owner, holding one immutable state (`snapshot`, `configuration`, `tree`) in three places:
**committed**, the last state the user's actions produced and the engine accepted, as the engine returned it (with the
compiler's types); **rendered**, the state on screen, which differs only while a query runs; and **pending**, the change
in flight. Presentation paints `pending ?? committed`; anything read off the rows on screen reads `rendered`. Nothing else
holds a copy.

- **Two kinds of change, told apart by the query itself.** A change whose state folds to the same query (colours,
  widths, formats, labels) is presentation: it commits at once, runs no query and cannot be refused. Any other change
  is a transaction: on success, committed and rendered become what the engine returned, with one undo step if the user
  made it; on refusal, everything repaints from committed; superseded by a newer change, it is dropped quietly (latest
  wins).
- **Overlapping work.** A change made on top of one still pending, then refused, reverts both and says which. Undo
  while a change is in flight cancels that change and nothing more; undo while an undo is in flight goes one step
  further. A change that changes nothing is nothing: no query, no step, no event.
- **Machine re-runs are not undo steps**; expanding a row sets it open, never toggles it; editors rebase their drafts on
  the owner's state; `dispose` stops everything the cube started.

The same transaction rules serve Ad Hoc mode (`state-owner.ts` is generic; each kind of state has its rules).

Sources: `docs/DATACUBE_LEG_B_STATE_OWNER_2026_09_28.md` (git history: docs/HISTORY.md).

### 5.3 A pivot is two plain queries

A pivot is two ordinary queries that legend-lite plans like any other, never upstream's dynamic `pivot()` with a
`cast`, which DuckDB expands into more than one statement (so a warehouse reader is refused it) and whose second stage
re-applies each aggregate over an intermediate (an average of averages):

1. **The pivot values**: the source with its row-stage columns and the cube's filter, the pivot keys selected,
   distinct, sorted in each key's direction, limited to `MAX_PIVOT_VALUES + 1` (`query.ts`, 500). More values than the
   cap is refused with a message naming the key; nothing is cut off silently. A NULL key is a value: it gets its own
   column, "(empty)", and the cells add up to the Total. Pinned values (`pivotValues`, an escape hatch normally absent)
   replace this query.
2. **One `groupBy` per level**: each pivot cell its measure over exactly that value's rows (`if(key == value, |x, |[])`
   inside the aggregate), the Total a column of the same query, so every aggregate is right -- average, count, median
   and weighted average included. It lowers to one `SELECT … GROUP BY` with `CASE WHEN` inside the aggregates, which a
   warehouse reader can run.

`plan.ts` (`planPivot`, `pivotFacts`) runs the first query; `query.ts` (`pivotValuesLambda`, `pivotColumns`) writes
both. Column types come from the compiler for both; nothing is learned from a result. A grouped pivot with N open
levels sends 1 + N queries.

Sources: `docs/DATACUBE_CUBE_PLAN_DESIGN_2026_09_27.md` (git history: docs/HISTORY.md).

### 5.4 Saved cubes and pages, and sharing

- **Our own versioned documents.** A saved grid is a `datacube.cube` document and a page a `datacube.page` document,
  each read whole: fields a newer writer added are carried, not dropped. Each reader takes exactly its current version
  (`cube-document.ts` `CUBE_VERSION` 2, `page-document.ts` `PAGE_VERSION` 6, since the user's ruling of 2026-10-17) and
  refuses any other by name. A legacy upstream `DataCubeSpecification` is meant to be read by a one-way translator;
  that is not built, and one is refused in words.
- **Stored through upstream's API.** The store is upstream's DataCube query API and record (`DataCubeQuery`, its
  `content` our document); the database enforces read access under the reader's identity, and the store does not
  gatekeep reads.
- **A cube never stores data.** A file is named by identity -- its name, format, size, SHA-256 and columns -- and asked
  for again on reopening (or read from a kept file handle in this browser); a sample is rebuilt from its seed.
- **A pivot is saved as intent** (its keys and aggregates), never the values a run found.
- **Pinned models, reconciled sources.** A page pins its model's revision (`page-model.ts`). A source whose columns
  drifted is reconciled and reported, not refused: a part that uses a column now gone is left out and named; a changed
  type is the compiler's, reported.
- **A share link is the page itself**: the saved page's JSON, raw-deflated against a preset dictionary frozen per
  version, base64url, after the `#` (`#p1.<data>`), never sent to a server. A link over 2,000 characters still works,
  and the person is told (`share/link.ts`). It is encoded, not encrypted: whoever holds it can read the page.

Sources: `docs/DATACUBE_SAVE_SHARE_2026_09_28.md` (git history: docs/HISTORY.md).

### 5.5 Charts, and the page model

- **Apache ECharts draws; our own spec is saved.** Charts are drawn by ECharts 6 (`datacube/package.json`), imported
  only by the drawing modules (`chart-render.ts` and its kin, the timeseries renderer). What is saved is DataCube's
  own versioned chart spec (`chart-spec.ts`, `CHART_VERSION` 2): plain JSON with no functions, so a saved chart never
  depends on the library's version and never needs `eval` to come back. Aggregation is always in the database;
  ECharts' own data filtering is never used, and a click or brush becomes DataCube's filter model and a new query.
- **The layout is built in-house** (gridstack was weighed and rejected).
- **Tiles are what, the layout is where.** A page's tiles are a flat registry (its views) and each sheet's layout
  refers to them by id, so filters, links and selections work on tiles and a tile moves between sheets and layouts
  untouched. The first design's layout tree (a free grid, splits, tabs) became the pages design's bands, sheets and
  stacks; its cross-source links by column became page filters by a class's property, through the model (§2.4).
- **A chart pinned to a grid follows its query, not its display**, and chooses its own grouping from its encoding.

Sources: `docs/DATACUBE_DASHBOARDS_DESIGN_2026_09_28.md` (git history: docs/HISTORY.md).

## 6. Proving it

DataCube's tests run its production paths: its unit tests in Node, its screen tests in jsdom over the real planner
and DuckDB-WASM, browser harnesses (`demo/verify-*.mjs`) in a pinned Chromium on every push, and the manual screenshot
tour (`docs/DATACUBE_TOUR.md`). The headless lane (`//datacube:headless_test`) runs DataCube's non-screen tests in V8
inside Python. The one-API ledgers (`test/one-api-ledger.test.ts`, `test/one-api-gestures.test.ts`) fail the build when
a command kind is not made by both the screen and Python.

## 7. The documents

| Document | What it holds |
|---|---|
| `docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md` | One app on every host: services, where queries plan and run (PlanAndRun), where pages and classes live, and how Python asks DataCube (§16) |
| `docs/DATACUBE_HEADLESS_PYTHON_DESIGN_2026_10_11.md` | DataCube's TypeScript run headless in Python: V8, the native library, the asynchronous boundary, the lane, packaging |
| `docs/DATACUBE_PAGES_DESIGN_2026_10_09.md` | Pages: every tile equal, bands, the page bar, sheets and stacks, the whole page saved |
| `docs/DATACUBE_CHARTS_DESIGN_2026_10_09.md` | The charts and dashboard features the ECharts gallery showed DataCube needs, in batches, and maps |
| `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md` | The two modes: prep makes classes; dashboards on the model; a grid is a query, not a table; Snap as a command |
| `docs/DATACUBE_ONE_API_PLAN_2026_10_21.md` | Every change a command, applied by one function, the same from Python (a build input) |
| `docs/DATACUBE_LIVE_EDITING_REDESIGN_2026_10_24.md` | Two editors of one page: the keeper's state, write ids, and the middle version built |
| `docs/DATACUBE_FLOW_MODE_2026_10_24.md` | Flow mode: decisions FL1-FL20, Alteryx's tools one by one, the roadmap M1-M8 |
| `docs/DATACUBE_TIMESERIES_DESIGN_2026_10_25.md` | The timeseries tile: its spec, expression language, queries, the Pure library and its conventions, the next phases |
| `docs/BI_AND_ETL_PLAN_2026_09_29.md` | The plan for DataCube as a BI tool (tracks A and B) and a visual ETL tool (track C), with the shared foundations (track F) |
| `docs/DATACUBE_ON_POSTGRES.md` | User guide: DataCube over a Postgres database |
| `docs/DATACUBE_TOUR.md` | The screenshot tour: a playbook for driving DataCube in a real browser |
| `datacube/docs/` | `AD_HOC_ANALYSIS.md`, `ENGINE_API_CONTRACT.md`, `UPSTREAM_CENSUS.md` |

The early designs folded into §5 -- and every other DataCube document removed from the tree -- are listed in
`docs/HISTORY.md`.
