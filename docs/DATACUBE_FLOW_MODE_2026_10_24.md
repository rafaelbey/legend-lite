# DataCube's flow mode: a visual ETL tool as a mode of DataCube (2026-10-24)

**The goal (the user, 2026-10-24).** A visual ETL and data prep tool that feels like a slick, modern Alteryx, with as
many of Alteryx's features as possible. legend-lite compiles every step and pushes it down to the database, with a
DuckDB copy (a "cache") when wanted. It is a **mode of DataCube**: the same logic and the same API, drawn as a
pipeline (a DAG) instead of a grid. The strict UI/API split DataCube keeps holds here too.

**Where this comes from.** Two earlier plans already point here, and neither was built:

- `docs/BI_AND_ETL_PLAN_2026_09_29.md` track C: a pipeline is a Pure program, the canvas edits it, the database runs it,
  and every node's preview is a DataCube on that node.
- `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md` §17.6: an Alteryx-like prep flow, in which "the boxes are the page's grids,
  and the lines are their join inputs. No second model."

This note puts the two together and records the user's decisions (§1). §3 to §5 are the design those decisions make.
Most of it is built (2026-10-25 and 2026-10-26): §4's status column and the notes in §5 say what, and what is not
(schedules, M8's F3, among it).

---

## 1. Decisions (the user, 2026-10-24)

Each was put to the user with its options, pros, cons and cost; the user took the recommended option every time ("Agree
with all"). The options not taken are in §7, so the reasons are kept.

**What it is**

- **FL1. A mode of DataCube, not a second app.** The flow is another way of drawing the page document. Every gesture on
  the canvas is a `PageCommand` or `GridCommand`, applied by the one function, as every other editor's is
  (`docs/DATACUBE_ONE_API_PLAN_2026_10_21.md`). So undo, save, share and Python come with it, and the canvas builds no
  command itself: screen code builds none (One API S8, ledger L11).
- **FL4. The flow is a sheet of the page.** A sheet's layout is a tagged type (`PageLayout`, `kind: 'bands'`,
  `datacube/src/page-document.ts:67`); a flow sheet is `kind: 'flow'`, its boxes placed on a canvas. Prep sheets and
  dashboard sheets are sheets of one page, and a dashboard sheet reads a box through a link.
- **FL5. A box is a grid that does one thing.** A Filter box is a grid with only a filter; a Summarize box a grid with
  only a grouping and its measures. Boxes chained by links give any order. No second model of a query.

**How boxes connect**

- **FL6. An input is live or a frozen copy, chosen per input.** A live input reads another grid's output as it is now
  (`{grid, port}`); a frozen input is a copy of what that grid was, today's `JoinSource` input
  (`datacube/src/cube-document.ts:170`). New joins are live. Joins saved before this read as frozen copies, so no saved
  page changes or stops opening. A live input whose grid is removed becomes frozen, and this is said.
- **FL7. Tools with several outputs have ports**, as Alteryx's anchors are (§3.3). Each port is a query worked out from
  its box; an unconnected port runs nothing. A line's record count is shown after a run, or when asked for, not kept
  live.
- **FL9. A gesture in the results pane adds a box** after the selected one (filter by a value, sort, group), so every
  box keeps doing one thing.

**Typing and checks**

- **FL2. A Cross Tab's columns are fixed by a cast.** A Cross Tab box's output is typed statically: `pivot(...)` is
  followed by `->cast(@Relation<(...)>)`, the pattern core already knows (`compiler/spec/typed/TypedPivot.java`). The
  cast's columns come from **Load values**: the values query (`pivotValuesLambda`, `datacube/src/query.ts:345`) run
  once and written into the cast. The person can then edit the list. Boxes downstream are typed from the cast.
- **FL3. A value the cast lacks is said, never dropped in silence.** A run compares the values query with the cast and
  names every value the cast does not list. With a cast alone, such a value would get no column and nobody would know;
  that is a fallback, and invariant 4 refuses fallbacks.
- **FL8. Each check has a mode: warn or refuse.** The checks: a Cross Tab's values against its cast (FL3); a source
  whose columns changed; the data-quality checks (Alteryx's Test tool, track C5). The mode is set on each check, as
  dbt's `severity` is. A new check is made with **refuse**, written into the document: the document always says the
  mode, and a check read with none is refused, never defaulted. A **preview** always shows the rows and marks the box
  (amber for warn, red for refuse) with the values or columns named and the fix offered ("Add to the cast"). A **run**
  stops before it writes anything on a refusal, and goes on with the finding in its log on a warning. A third mode,
  **follow** (each run reloads the values, rewrites the cast and recompiles what is downstream), is for later.

**Data, databases and Pure**

- **FL10. A Cache box moves data between databases, visibly.** A join over two databases is refused with an offer to
  put a Cache box on one side: a box that copies its input into a chosen database, built on Snap (`snap` and `live`,
  the model design's §18.8). Nothing is copied without a box saying so. First target: the host's own DuckDB (the tab's,
  or Python's on a page Python serves).
- **FL11. The cache lasts as long as its session**, as a snap does today (§18.8, item 7) and as Alteryx's Cache and Run
  does. A reopened flow reads its sources again, and says so.
- **FL12. The page document is the source; Pure is imported, not edited in place.** Each box's Pure is shown and can be
  exported. **Import Pure** turns a lambda into boxes; whatever does not become boxes becomes a **Pure box**: a box
  whose query is a Pure expression over its inputs, checked by the compiler. The Pure box is also "filter by a Pure
  expression" (§17.6, item 6). Editing the Pure and having the canvas redraw (track C2's "both directions") is for
  later, if wanted.

**Scope and timing**

- **FL13. The first version writes no tables.** Its outputs are the Cache box, Snap and the grid's exports. F1 (`write`
  into a table: create, replace, append, merge by key) is built in core in parallel, as a slice of core's own line, not
  inside DataCube's. Output boxes come when it lands. *Since (2026-10-25): the writes line landed store writes on main
  (`docs/WRITES_DESIGN_2026_10_18.md` legs 2 and 7: `write`, `update`, `delete` and `merge` over a store table on
  DuckDB, H2 and Postgres, and write plans run by the model-free runner), so F1 is no longer waited for; Output boxes
  are the roadmap's M6 (§5).*
- **FL14. Transpose is composed, not a new native.** One `select` per chosen column (the keys, that column renamed to a
  name and a value), the selections concatenated. It runs on every database and needs no core change. The value
  columns must share a type; where they do not, the compiler refuses in its words, and a cast is offered.
- **FL15. No Python or R boxes in the first version.** They compute outside the database ("Java orchestrates, the
  database executes"). A Python box run by the page's Python host, as a named exception ruled under
  `docs/TENET_CHARTER.md`, can be asked for later.
- **FL16. The logic now, the canvas after D19.** The link and port commands (pure functions: apply rules, readers,
  rebase, Python builders), Transpose and core's F1 start now, beside D19. The canvas, a second editor of the page,
  waits for D19 (`docs/DATACUBE_ONE_API_PLAN_2026_10_21.md`: the client side of the single writer), so it is not built
  on the races between two editors that D19 fixes.

**Decisions (the user, 2026-10-25)**

- **FL17. The roadmap's order stands** (§5), and so does the reuse: each box's settings are DataCube's own editors.
- **FL18. The Filter window, sleeker, the same view.** As the Join window was rebuilt in the Create a Class window's
  style and the Properties window cleaned up, the Filter window is restyled the same way, keeping its tree of
  conditions and groups as it is: the same view, slightly sleeker. Its one editor (`datacube/src/ui/filter-editor.ts`)
  is the grid's Filter window and the flow's Filter box alike.
- **FL19. Union goes back into DataCube.** The flow's Union (M2) is built as DataCube's own, so a grid can union as it
  can join. DataCube has half of it already: the Join window's **Append** form (`join-how.ts`, `form: 'append'`) puts
  two grids' rows together, columns matched by name, a column one side lacks empty on its rows. What Union adds, to the
  grid and the flow both: a Union entry of its own beside Join, and any number of inputs. Proposed, as Alteryx's Union
  offers it: matching by position as well as by name.

**Decision (the user, 2026-10-25)**

- **FL20. Python's names for flows** (D6): `page.flow()` adds a Flow sheet; `flow.grid(...)` an Input box, with
  `page.grid`'s arguments; `flow.grid(input=box)` (and `page.grid(input=grid)`) a box after another, reading its rows
  live, as `page.grid(cls=..., query=..., warehouse=...)` names its sources; `box.place(x, y)` a box's place. Every
  tool's settings are the grid's own methods (`filter`, `calculate`, `sort`, `group`, ...): a box is a grid. *(The
  same day: `rows=` was proposed first and approved, then found taken -- `page.grid`'s `rows=` is its row groups -- and
  the user chose `input=`, Alteryx's word for what a tool reads.)*

---

## 2. What exists, and what the flow reuses

| The flow needs | What DataCube has today |
|---|---|
| Types on every box, as it is connected | The tab's planner (`//wasm:planner`), the same compiler as the server: every query's relation type, with no server |
| A preview of any box or port | A grid: the selected box's rows shown as a DataCube, as Alteryx's results pane shows them |
| Edits as data | `GridCommand` (`datacube/src/grid-commands.ts`) and `PageCommand` (`datacube/src/page-commands.ts`), one apply function each, a generated Python builder each (D17) |
| The tools' logic | The filter editor (`FilterNode`), calculated columns checked by the compiler (row and window), aggregates (`wavg` and `unique` included), sorts, joins (`join.ts`: any condition, as of, keep my rows with a match or with none, append), Expand and JSON fields |
| A cache | Snap: a grid's rows copied into a store and read from there (§18.8 of the model design: Snap is a command) |
| A second way of drawing the same state | Ad Hoc mode (`datacube/src/adhoc/mode.ts`): "turns a gesture into ONE session call", and "nothing here computes a number" |
| Sheets of one page | `PageSheet` and its tagged `PageLayout` (`datacube/src/page-document.ts:67`) |
| Output | The grid's exports, charts, KPIs and pages. Not a table write (FL13) |

---

## 3. The design

### 3.1 Live inputs: the one missing primitive

Today a join keeps its two inputs by value: "an input is what its grid was when it was joined". So the page is not a
graph: an edit to a grid upstream does not reach the grids that read it.

The flow adds the live input (FL6): the output of another grid, `{grid: 'grid-3'}`, or one of its ports,
`{grid: 'grid-3', port: 'false'}`. It fits the model design's §18 rule ("a grid is a query, not a table"): a live input
denotes the upstream grid's output query, and its leaves are that grid's leaves. Nothing else needs to know what kind
of source a grid has.

What comes with it:

- **A cycle is refused** by the page command that would make it.
- **Grids open in dependency order** (class grids already open last, §16.4).
- **A removed upstream grid** turns the live input into a frozen one, a copy of what that grid was, and this is said.
- **Freeze and unfreeze** are commands: freezing copies the upstream's source and query into the input; unfreezing an
  input whose grid is still on the page makes it live again.
- **A rename upstream** is followed downstream by the page's rebase (`datacube/src/page-rebase.ts`).
- **A box's output** is its rows at their finest level: `classLambda` (`datacube/src/query.ts:380`), what a join input
  already reads. A grouped grid outputs its groups, one row per group with its measures beside its keys: Alteryx's
  Summarize. A pivoted grid outputs its cast (FL2); `classLambda`'s refusal of a pivot is Create a Class's rule (a
  class's properties are fixed), not a link's.

### 3.2 One box, one tool

A grid applies its steps in a fixed order: calculated columns, then the filter, the grouping, group-level calculations,
the pivot and the sorts. Alteryx users expect one tool per box, in any order. Each tool box is therefore a grid that
does one thing (FL5): a filter after a summarize is two boxes. The canvas never shows the word "grid"; the results pane
is the grid. A grid stores only what differs from the defaults (`CubeDocument.configuration`), so a one-tool grid is a
small document.

Tools that are not yet a grid's feature (Union, Sample, Unique, rename, Transpose, the Pure box) each become a grid
feature or a source kind, one at a time, each as its own command kind (§3.5).

### 3.3 Ports

| Tool | Outputs | Each output's query |
|---|---|---|
| Filter | True, False | the filter; its negation |
| Join | L, J, R | left rows with no match; the join; right rows with no match (join.ts has both "no match" forms) |
| Unique | Unique, Duplicates | the first row of each key; the rest (a row number over the key) |
| Create Samples (later) | Estimation, Validation, Holdout | three random slices |

Inputs are anchors too, as in Alteryx: a Join's L and R are its two inputs; a Union takes any number. Each port's query
is worked out from its box: nothing is stored per port. A link that names no port reads the box's main output. Clicking
a port shows its rows in the results pane.

### 3.4 The flow sheet's own state

- **The screen:** the palette on the left, the canvas in the middle, the selected box's settings on the right (the
  editors DataCube has: filter, column, join, pivot), the results pane below.
- **A box's place on the canvas** is the sheet's layout, set by a page command (as a board sheet's `layout` is), so
  undo, save, share and Python treat it like any other edit. Auto-layout is a command too.
- **Annotations and containers** (Alteryx's Comment and Tool Container) are the sheet's state the same way.
- **The selected box** is the screen's, not the page's (as a selected cell is today).

### 3.5 New command kinds ship whole

Each new command kind (a live input, freeze and unfreeze, a box's place, a check's mode) ships with its apply rule, its
JSON reader and writer, its rebase arm, a UI producer, a Python builder and a parity row (One API rule 6). The ledger
L8 in `datacube/test/one-api-ledger.test.ts` enforces it: a kind the screen or Python does not make, or a source kind
Python cannot read and add, fails the build. Its lists (`L8_KINDS`, `L8_SOURCES`) are the gaps, so a kind shipped
whole adds nothing to them; an entry there is a gap left, which only the user's dated sign-off allows (D9).
*(Corrected 2026-10-25: this said every new kind raised `L8_KINDS`.)* A new source kind also joins `PYTHON_ADDS`,
the headless function by which Python adds a grid over it, with its test.

---

## 4. Alteryx's tools, one by one

How to read the status column:

- **Built**: DataCube does it on a grid today. The flow only draws it.
- **Step**: DataCube work only (a preset, a port, a command) over what the compiler already has. Where it names a Pure
  function, the function is in the platform's registry (`core/src/main/resources/com/legend/builtin/native-claims.tsv`)
  and DuckDB's dialect renders it; other databases are checked when the step is built, not assumed here.
- **Core**: needs a change in core first.
- **Later**: possible, not in the first slices.
- **Out**: refused, with the reason.

### In and out

| Tool | Status | How |
|---|---|---|
| Input Data | Built | A grid's sources: a file, a remote file, a warehouse table, a class, a saved query, a frame |
| Text Input | Built (2026-10-25) | A small table typed in or pasted from a sheet, written as a CSV file: an Input over it, as a file dropped on the canvas is. Pure's `#TDS` literal remains the way to keep one in a model |
| Browse | Built | The results pane: the selected box's grid, its Profile, its Chart |
| Output Data (to a table) | Built (2026-10-26) | An Output box's table kind: created, replaced, appended, its rows deleted first, or merged by keys -- the planner's execution plan of the store write, run by the host's plan runner in one transaction ("Built, an Output into a table") |
| Output Data (to a file) | Built (2026-10-25) | An Output box: its input's every row (the columns it shows) streamed from the database and written as CSV, by Write now and each time the flow runs; Python's `grid.output(name)` and `flow.run(folder)`. The grid's exports still write the rows shown |
| Data Stream In / Out | Built (2026-10-26) | The Cache (FL10, FL11): a mode of any box, its rows copied into the host's database on Snap and read by the boxes after it ("Built, the Cache") |
| Date Time Now | Built | `now()` and `today()` in a Formula |
| Directory | Out | It lists a file system, which is no relation a database holds |
| Dynamic Input | Built (M8) | A Union that keeps the rows of the one input a String parameter of the page names, picked as it runs ("Built, Dynamic Input") |

### Preparation

| Tool | Status | How |
|---|---|---|
| Filter (True, False) | Built (2026-10-25) | The grid's filter; the False port (§3.3, FL7): the rows its condition leaves out, empties among them |
| Formula | Built | A calculated column, checked by the compiler as it is typed |
| Multi-Row Formula | Built | A window column: `lag`, `lead`, `first`, `last` |
| Multi-Field Formula | Built (2026-10-25) | One expression over `$v`, each column's value, parsed by the compiler: a calculated column of each (`<column><suffix>`), or in place, as Alteryx's "change existing fields" |
| Select (choose, rename, retype) | Built (2026-10-25) | Its own window: each column kept or left out (hidden), renamed (its display name, which a flow's box hands on as the relation's `rename`) and retyped (a calculated column of its conversion -- `parseInteger`, `toString`, `round`, `toFloat`... -- its column hidden, it handed on by the column's name) |
| Sort | Built | The grid's sorts |
| Sample (first, last, every Nth, N per group) | Built (2026-10-25) | The first or last N of all the rows, the grid's Top N; of each group, all but the first N, or one of every N, a row number (`rowNumber`, per group) and a filter on it, hidden -- `mod` of it for one of every N |
| Select Records | Built (2026-10-25) | A Sample's way: a row number and a filter of its ranges (`1-10, 20, 30+`), or-ed |
| Random % Sample | Built (2026-10-25) | A Sample's way, the same rows for a seed: a row number in an order of every column, its chance the database's `hashCode` of it and the seed, kept under the percent |
| Unique (Unique, Duplicates) | Built (2026-10-25) | A row number per key (`rowNumber` partitioned by its columns, in an order), a filter keeping the first, the number hidden; its Duplicates the filter's False port (§3.3), drawn U and D |
| Record ID | Built | A window column: `rowNumber` |
| Tile | Built | A window column: `ntile` |
| Data Cleansing | Built (2026-10-25) | Calculated columns, a `<column>_clean` each: empties made blank (`coalesce`), punctuation out and repeated spaces one (`regexpReplace`), `trim`, `toUpper` / `toLower` (§17.6). In place, as Alteryx's: each handed on by its column's name |
| Imputation | Built (2026-10-25) | `coalesce` with a value, or with a window column of the column's average, median (2026-10-26), smallest or largest across every row (hidden): a `<column>_filled` each |
| Date Filter | Built | Its own tool: a date column in a range relative to today, or between two days (date-ranges.ts) |
| Filter's Custom filter | Built (2026-10-25) | Its own tool, Custom Filter: a condition written in Pure over `$x`, parsed and typed by the compiler (a Boolean), a hidden calculated column and the grid's filter on it alone; its T and F (FL12's "filter by a Pure expression") |
| Generate Rows | Built (2026-10-26) | Each row repeated for each whole number from one to another (numbers given or its columns, by a step), or each day between two of its dates: an exploded calculated column of the database's `range`, a day `adjust`ed by each |
| Auto Field | Out | Every column already has its type from the compiler; there is nothing to guess |
| Create Samples | Built | A Sample's two outputs: its rows (S) and the rest (R), its False -- a split by a seed |
| Multi-Field Binning | Built (2026-10-26) | Binning: number columns each into N bins of equal width over its range -- the grid's bins preset, the database placing each value; equal records are the Tile's |
| Oversample | Built (2026-10-26) | A Sample's way: every row of a value of interest kept, the others numbered in the order of the database's hash of each and a seed and kept while within the share that makes the value N% of the rows (a window count, all hidden) |

### Join

| Tool | Status | How |
|---|---|---|
| Join (L, J, R) | Built (2026-10-25) | `join.ts`: inner, left, right, full, any condition, as of. Its L and R ports are each side's rows matched against the other's: anti-joins (§3.3) |
| Join Multiple | Built, once linked | Joins chained by live inputs |
| Union | Built | A join's append, of any number of inputs (`JoinSource.more`) |
| Append Fields | Built (2026-10-25) | Every row with every row: a join, matching rows only, on no condition -- the Join window's Every pair |
| Find Replace | Built (2026-10-25) | A Join box, all of mine on my column equal to theirs, its column then handed on as theirs where found (`coalesce`, in place). A key theirs repeats matches its first row only (2026-10-26, C13): the join's `once` cuts theirs to the first of each key, by a row number the database gives in an order of their other columns, rising -- the Join window's "Each of mine once", ticked for a Find Replace; Python's `page.find_replace` the same |
| Fuzzy Match | Built (2026-10-25) | A join row comparing by similar text (`~`): both a value, at most N edits apart by `levenshteinDistance`; a Fuzzy Match box opens the Join window at it. Its score (2026-10-26): a column of each matched pair's likeness, 0 to 100, from its edits apart for the longer text's length, the database's |
| Make Group | Out for now | Groups made from pairs need a transitive closure, which one relation cannot say |

### Parse

| Tool | Status | How |
|---|---|---|
| Text To Columns (to columns) | Built (2026-10-25) | `splitPart`, one calculated column per part |
| Text To Columns (to rows) | Built (2026-10-25) | `split`, exploded: one column, each row once a part |
| RegEx | Built (2026-10-25) | `regexpLike` (Match), `regexpReplace` (Replace), `regexpExtract`'s first (Extract), one tool; Tokenize (2026-10-26), each match its own row: `regexpExtract`'s all, exploded |
| DateTime | Built (2026-10-25) | One tool, three ways: a date to its period (the grain preset), a date's part (`year`, `monthNumber`, `dayOfMonth`, `weekOfYear`, `hour`), text read as a date by a format (`convertDateFormat` / `convertDateTimeFormat`: strptime); a date written as text by a format (2026-10-26: `format`'s `%t{...}`, the engine's tokens the compiler translates to strftime) |
| JSON Parse | Built | Its own tool: the column editor's JSON fields (`datacube/src/ui/json-fields.ts`) and Expand, on the box's JSON column |
| XML Parse | Out | No XML function in the platform |

### Transform

| Tool | Status | How |
|---|---|---|
| Summarize | Built | A grid's grouping and measures: sum, count, average, min, max, median, quartiles, standard deviation, variance, concatenate (`joinStrings`), weighted average, unique -- set in Alteryx's window, a row a column (2026-10-25) |
| Cross Tab | Built (2026-10-25) | The grid's pivot; Load values pins its values, so its columns are fixed and a box after it reads them, each named by its value (FL2); a value its rows gain that the pin lacks asked of the database and said on its box, a run stopped by it in refuse mode (FL3, FL8) |
| Transpose | Built (2026-10-25) | An exploded column over the list of the columns' names (`lateral(...->flatten(~Name))`), a column choosing each row's value by its name (`if`), every column but the keys hidden; columns of one kind of value |
| Running Total | Built | A window column, running frame |
| Weighted Average | Built | `wavg` |
| Count Records | Built (2026-10-25) | One row, its count: a column of one value grouped by and hidden, a 1 for each row summed (`count-records.ts`) |
| Arrange, Make Columns | Later | Layout tools for reports; the pages do most of what they are for |

### Investigate and report

| Tool | Status | How |
|---|---|---|
| Field Summary, Basic Data Profile | Built (2026-10-25) | **Profile** (§17.6): per column, the count, empties, distinct values, smallest and largest, one row of aggregates the database computes; a column opened, its ten most frequent values (2026-10-26): grouped by it and counted by the database, the most first, of as many the smallest, an empty a value too -- asked when opened, kept until the box's rows change (`profile.ts topValuesLambda`) |
| Frequency Table, Histogram, Scatterplot | Built | Charts on a box's grid: bars over bins, scatter, box plot |
| Pearson Correlation | Built (2026-10-25) | In every box's Profile, under its columns: each pair of its measures' `corr`, a matrix asked of the database as one row of aggregates |
| Charts, Table, Layout, Render, Email | Built | Charts, KPIs, pages, exports and the email draft |

### Developer, apps, macros and the canvas

| Tool | Status | How |
|---|---|---|
| Comment, Tool Container | Built (2026-10-25) | The flow sheet's own state (§3.4): a comment a text tile; a container one with a frame, its boxes moved with it (`layout/flow.ts containerMoves`, Python's `text.place`) |
| Test | Built | Checks as a box (no empty value, no value repeated, a number within bounds, a count of rows within bounds, values one of a list, a text matching a pattern), counted by the database; a mode that stops a run (FL8) |
| Block Until Done | Built (M8) | A mode of any box: in a run, the boxes after it run, and their Outputs are written, once every Output before it is ("Built, Block Until Done, Message and the run history") |
| Message | Built (M8) | A mode of any box: its words in the run's log (`{rows}` its rows counted) each run, when it has rows or when it has none; a note, a warning, or an error that stops the run as a failing Test does |
| Base 64 Encoder | Built | `encodeBase64` in a Formula |
| Dynamic Rename, Select | Built (2026-10-25) | Select's window (Preparation, above); Dynamic Rename, a prefix and a suffix given each kept column in sight |
| Dynamic Replace | Built (2026-10-26) | Rules read from another box's rows, a column, a condition in Pure and a value each, pinned as a column per column they name: a chain of `if`s, the column itself last, handed on in its place (`dynamic-replace.ts`) |
| Interface tools (an analytic app's inputs) | Built (2026-10-26) | The page's parameters (`page-parameters.ts`): a text, a number, a day, true or false, or one of a pick-list, set in the parameters bar and read by any Pure on the page as `$name`, bound where each query is sent. An app's value set before a run is the bar's; core's execute takes parameter values too (`PureV1Api`), which a scheduled run can use (F3) |
| Standard macro | Built (2026-10-26) | A sub-flow saved as one Pure function of its input's rows (a box's menu, Save as Macro…: its own steps and those of the boxes before it, each read off its grid's query), kept on the page and used after any box from the palette's Macro: a Pure box of it, named after it. Its parameters are the page's it reads (`macros.ts`) |
| Batch and iterative macros | Out for now | A loop is not one query; it would be orchestration |
| Python, R | Out in v1 | They compute outside the database (FL15) |
| Download, Run Command | Out | Not a database's work |

### Not ours

- **Spatial, predictive, time series, prescriptive, address and demographic tools:** a different product. A function
  the database offers (a spatial or statistical one) can be added as a native when asked for.
- **Connectors to SaaS applications:** out of scope, as the BI and ETL plan's §5 says.
- **The In-Database palette:** every tool here runs in the database, so there is no separate palette for it.

---

## 5. Roadmap: simple first, then built up (2026-10-25)

D19 landed on main (neema2/legend-lite#71), so FL16's wait is over and the canvas can start. The user's three rules
for all of it (2026-10-25):

1. **Reuse DataCube's logic and APIs**, so there is one set of rules, never a second.
2. **The API stays separate from the UI**: every gesture a command, applied by the one function, the same from Python.
3. **Close to Alteryx's experience, features and workflow**, but nicer, more modern and sleeker.

Every milestone ends usable: something a person does from start to finish, saved, shared, and the same from Python.

### M1. A first flow: an Input, five tools, Browse

- **What a person does:** adds a Flow sheet; drops a file onto it, or picks any DataCube source, as an Input; adds
  Filter, Formula, Select, Sort and Summarize after it; clicks any box to see its rows; saves, reopens and shares it.
- **The canvas:** boxes and lines. A tool is dragged from the palette onto the canvas, onto a box's output, or onto a
  line to insert it there. Boxes move, are deleted, and every step undoes and redoes.
- **Settings:** each box's in the pane on the right, made of DataCube's own editors: the filter editor (Filter), the
  column editor (Formula), the shown columns (Select), the sorts (Sort), the grouping and its measures (Summarize).
- **Results:** the selected box's rows below the canvas, as a full DataCube, live: there is no Run button. Only the
  shown box queries; every other box is a document, not a running grid (demand-driven work).
- **Types:** each box says its output columns as it is connected; a box the compiler refuses is red, in its words.
- **Underneath:** the `rows` source (a grid over another grid's rows, §3.1) with live inputs; the `flow` layout kind
  (FL4); a box with no running grid until it is shown.
- **Not yet:** joins, ports, writes.
- **Done when:** a browser harness builds Input, Filter, Formula and Summarize by drag and drop alone, previews each
  box, saves, reopens and shares the flow; Python builds the same flow and reads the same rows.

### M2. Combine: Join, Union and ports

- A tool dropped onto a line, inserted there (moved from M1: a link changed by a command, with Freeze and Unfreeze).

- Join with its L, J and R ports; Union of any number of inputs; Append Fields; Find Replace; Fuzzy Match.
- Ports (FL7): Filter's False, and Unique with its Duplicates.
- Live or frozen inputs, with Freeze and Unfreeze (FL6).
- **Nicer than Alteryx:** join keys suggested with their reasons, and what a join will do counted before it is
  applied (join.ts `suggestedKeys` and join-open.ts `summaryCounts`, both built).
- **Done when:** two inputs are joined, each port is browsed, and an edit upstream reaches the join and every box after
  it.

### M3. Prepare and parse

- Preparation: Sample, Random % Sample, Record ID, Tile, Multi-Row Formula, Running Total, Multi-Field Formula, Data
  Cleansing, Imputation, Date Filter.
- Parse: Text To Columns, RegEx, DateTime, JSON Parse.
- Most are presets of the calculated columns and window columns DataCube has (§4).
- **Done when:** each tool has a harness case over a fixture, its rows checked.

### M4. Reshape

- Cross Tab, with Load values, the cast, its check and the check's mode (FL2, FL3, FL8); Transpose, composed (FL14).
  Built (2026-10-25): Load values (its values pinned), its check (a value its pin lacks, said), Stop Run On Failure.
- The check modes on screen: amber and red boxes, Add to the cast. *Built: amber for a check that warns, red for one
  that stops a run (2026-10-25); Add to the cast (2026-10-26): a Cross Tab's values its pin lacks given columns after its
  own, one step (`pivot.pin`, Python's `grid.pin_pivot` with the longer list).*
- **Done when:** a Cross Tab whose data gains a value is marked in a preview and refused by a run in refuse mode.

### M5. Understand the data

- Profile on any box or port: the count, empties, distinct values and top values (§17.6). *Top values built (2026-10-26).*
- Record counts on lines after a run (FL7); results-pane gestures that add a box (FL9); a box's Pure and SQL shown;
  charts on any box.

### M6. Output and run

- **Output Data to a table:** create, replace, append and merge by key, on the store writes now on main (FL13's note).
  *Built (2026-10-26): §5's M5 and M6 steps, "Built, an Output into a table".*
- The Cache box (FL10); the grid's exports from any box. *The Cache built (2026-10-26) as a mode of any box: §5's M5
  and M6 steps.*
- **Run:** the whole flow in order, with a run log; check modes enforced (FL8); a dry run (the plan and row counts,
  nothing written).
- **Done when:** a flow writes a table in one transaction, and a failed check writes nothing and says why.

### M7. Big flows

- Comments and containers that collapse, auto-layout, a minimap, search, a command palette, copy and paste of boxes,
  keyboard operation, accessible announcements.
- Import Pure and the Pure box (FL12). *The Pure box built (2026-10-26, its note above); Import Pure built the same day (§5's M5 and M6 steps, "Built, Import Pure").*

### M8. Production (platform work)

- Schedules and run history (F3); parameters and analytic apps (F5); standard macros (a sub-flow as a function with
  parameters); Dynamic Input; FL15's Python box, if asked for. *Parameters built (2026-10-26): §5's M5 and M6 steps,
  "Built, a page's parameters"; standard macros the same day, "Built, standard macros"; Dynamic Input the same day,
  "Built, Dynamic Input"; Block Until Done, Message and the run history the same day ("Built, Block Until Done, Message and
  the run history"). Schedules are not built: F3's scheduler waits on where it lives (docs/BI_AND_ETL_PLAN_2026_09_29.md,
  open decision 5).*

### What makes it nicer than Alteryx, all the way through

- No Run to see data: every box previews live, computed in the database.
- Types and the compiler's errors live, as boxes are connected and settings typed.
- Every box is a full DataCube: pivot, chart or export at any step.
- Undo for everything, a share link for every flow, the flow as diffable Pure.
- The same flow from Python, by the same commands.
- One design language (`docs/DATACUBE_UI_REVIEW_2026_10_24.md`, now in git history: `docs/HISTORY.md`), dark mode,
  the keyboard first.

### M1's own steps, each a commit with its tests

1. **The `rows` source and live inputs in the cube document:** read and written; resolved against the page (a cycle
   refused, a missing grid said); its leaves; the order a page opens its grids in.
2. **Its base query** over the host's tables, on the tab and on Python's host (headless), and the L8 source rows.
3. **The `flow` layout kind** and its page commands: a box placed, moved and removed; every reader of a layout taught
   it.
4. **The canvas**, the palette, the settings pane and the results pane.
5. **The five tools' settings.**
6. **Python's methods**, by the names agreed (D6).
7. **The browser harness** and a tour step.

### M1's design, as the code shapes it (2026-10-25)

Read while building step 1, against main at `1e91768d`.

1. **A box is a document until it is shown.** M1 runs only the shown box. So the box before it is usually not an
   open grid, and a box's base cannot be taken from an open grid upstream. A box's base is worked out from its chain's
   first source down, one rule at every step: **an input's rows** (join-open.ts `inputRows`, the rule a join's input
   already followed, now shared) over the base of what that input reads. Python's host already works a base out this
   way, per source (headless host-tables.ts `onHost`, grid.ts `gridBase`), so there the rule is one more arm.
2. **The tab's openers split in two.** Each kind's opener in `datacube/src/app/sources.ts` today both opens a source and
   makes its grid. A box needs the first half alone: the base (its relation, its columns, the planner and plane it is
   read through) with no grid made. The opener becomes "open the base", then "make a grid over it"; every kind keeps
   its one opener.
3. **The page holds a box's document while it is not open.** The page today reads each grid's document off its open
   grid (page-app.ts `#seen`), and keeps the documents of the grids it could not open (`#withUnopened`). A box not
   shown is the second case by design: its document lives in the page's state and is edited by commands, and its grid
   is made when it is shown and let go when it is not.
4. **A link is resolved, never guessed.** Whatever asks what a grid reads (its leaves, its base) asks a source whose
   live inputs are resolved against the page first (live-inputs.ts `resolvedSource`); `leavesOf` refuses an unresolved
   one rather than answering for it. A page whose link names no grid of it, or goes round, is refused when read.
5. **The Join window's copy holds no link.** A grid joined is copied with its links resolved (page-commands.ts
   `joinInputOf`, now live-inputs.ts `gridInput`, the one rule for a grid as an input): a frozen input never changes.

> Cut in the 2026-11 docs cleanup: the as-built records of M1's steps and of M2 to M8 (2026-10-25 to 2026-10-26) -- every "Built, ..." paragraph that §4's table and §5's notes name ("Built, the Cache", "§5's M5 and M6 steps", ...), the roadmap audit of 2026-10-26 with the limits it left named, and M2's open list. Full text: `git show de2745351:docs/DATACUBE_FLOW_MODE_2026_10_24.md`.

---

## 6. Still open

1. **Whether legend-pure has an `unpivot`.** Nothing in this repository shows one, and no legend-pure checkout was in
   the session that wrote this. If it has one, Transpose uses it instead of being composed (FL14).
2. **Python's method names** for flows, put to the user before they are built (D6), at M1's step 6.
3. **An Output box in the tab:** the tab plans with its WebAssembly planner and runs on its own DuckDB; whether its
   runner takes a write plan's `Update` and `Sequence` nodes is to be read before M6. *Read (2026-10-25): no, and
   neither does Python's.* A write is Pure (`->write(#>{db::DB.T}#)`, `update`, `delete`, `merge` by key; a replace is a
   `delete` then a `write`), into a table a Database declares and that exists; planned, it is an execution plan --
   a `Sequence` of `Update` steps, `Check`s and a `TextResult` (`core/.../ExecutionPlan.java`, `PlanMaker.program`) --
   run in one transaction by Java's `PlanRunner` alone. The query path both hosts plan through is the single-SQL one:
   `Plan` is `{sql, columns}` (`engine-client/src/relation-type.ts`), the tab's runner and Python's each run one
   `plan.sql`, and the lowering refuses a store write in a query's plan (`Lowerer.java`, `TypedWrite`). So an Output to
   a table needs, in each host, the execution plan from its planner (a wasm and a native export of `PlanJson`) and a
   runner of its steps (Python's: a port of `PlanRunner.sequence`; the tab's: one in TypeScript, its connection
   binding no parameters today), over a model that declares the target beside the sources. A host operation
   (`CREATE OR REPLACE TABLE ... AS` the box's SQL, as Snap copies) would write one today, but outside Pure: not the
   way, by the execution tenet. A table the tab writes lives in its in-memory DuckDB and goes when the tab does.
   *Settled (2026-10-26): both hosts get the execution plan from their planner (the `executionPlan` kind) and run it by
   one runner in TypeScript (`engine-client/src/plan-runner.ts`), the tab binding by DuckDB-WASM's prepared statements
   and Python's headless DataCube by a host call; only the table's making (`create`, `overwrite`) is a host operation,
   as Snap's copy is. §5's M5 and M6 steps, "Built, an Output into a table".*

---

## 7. The options not taken (2026-10-24)

Kept so a later reader knows why. Sizes: S days, M a week, L a few weeks, XL a month or more.

| Decision | Not taken | Why not |
|---|---|---|
| FL4 | A whole-page mode, like Ad Hoc | Dashboard grids and prep boxes in one picture; one-tool grids as tiles on dashboard sheets |
| FL4 | A separate flow document | A second open, save and share path and a second Python surface, against FL1 (L) |
| FL5 | Step records, the grid only previewing | A second model of a query beside the grid's: the duplicates the One API program removed (L-XL, and upkeep) |
| FL5 | Grids that absorb neighbouring tools | Boxes merge and split without being asked |
| FL6 | Live inputs everywhere, joins included | Saved joins change shape, and older documents are not read (the user, 2026-10-17) |
| FL6 | Live in flows, copies for the grid's Join… | Two kinds of input people must tell apart, with no choice between them |
| FL7 | One output per box | Unfamiliar to Alteryx users; settings repeated |
| FL7 | Counts kept live | One count query per line per refresh |
| FL8 | One mode per flow | Cannot be strict on one check and lenient on another |
| FL8 | Warn by default | A table can be written short, said only in a log |
| FL8 | Follow mode now | The SQL that runs differs from the plan reviewed; two queries a run (M); later |
| FL9 | A gesture edits the selected box | A box stops doing one thing |
| FL10 | Refusal alone | Alteryx users expect a way through |
| FL10 | An automatic copy of the smaller side | Data moved without anyone seeing it (M-L) |
| FL10 | DuckDB attaching the other database | DuckDB doing the other database's work; credentials (L) |
| FL11 | The cache saved with the page, or in the warehouse | Stale data and governed rows in a browser (M); the warehouse needs F2's grants (L) |
| FL12 | Pure read-only, no import | No escape hatch for what the palette cannot say |
| FL12 | Two-way Pure now | Any Pure turned back into boxes, layout kept across edits (XL); later, if wanted |
| FL13 | F1 before the first version | Holds the first version on core work (L) |
| FL13 | Files only, through DuckDB's `COPY` | No tables |
| FL14 | A new `unpivot` native | Not shown in Pure's relation API: a platform invention, a `SEMANTICS_REGISTER.md` row, against signatures verified from legend-pure |
| FL15 | A Python box now | A named exception to the tenet; only on pages Python serves; nothing pushes down through it |
| FL16 | Wait for D19 entirely | Slower, with nothing gained for the pure-function parts |
| FL16 | Everything now | The canvas built on the races D19 fixes |
