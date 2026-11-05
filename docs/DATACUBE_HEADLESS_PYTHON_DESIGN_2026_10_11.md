# DataCube headless in Python: DataCube's own code, embedded, with no WebAssembly (design, 2026-10-11)

**Status: agreed by the user, 2026-10-11. Reviewed by the Plan Gen / Exec Split line (yes, with one gap, closed in
§3: the plan of a query over a held model natively) and by the Compiler Rewrite line (no blocker; the lane in
`//gates:warehouse`, and the build's shape, §6). Steps 0, 1, 2 and 4 are built (2026-10-11; their as-built notes, §11 to §14, were cut in the 2026-11 docs
cleanup).**
The user, on the measurement: "this is so awesome lets do it"; on §10: "yes one one package, v8 for sure, yes do this
before create*; yes rename would helpful". Builds on `docs/DATACUBE_PYTHON_SHOW_DESIGN_2026_10_08.md` (the engine in Python, the notebook cube),
`docs/DATACUBE_PYTHON_PAGES_DESIGN_2026_10_09.md` (pages built in code; both in git history, docs/HISTORY.md) and `docs/DATACUBE_MODEL_DESIGN_2026_10_10.md`
(§13's model plan; §15.5's Python). Evidence: `docs/datacube-headless-evidence-2026-10-11/` (in docs/HISTORY.md).

## 0. In one paragraph

DataCube's meaning lives in its own TypeScript, which turns a grid's settings into Pure, runs a grid's levels, totals
and pivots, and writes exports. That code needs two things, a **planner** and a **database**. It does not need a
browser, and it does not need WebAssembly. In the browser both happen to be WebAssembly: legend-lite's planner built by
TeaVM, and DuckDB-WASM. In Python they are legend-lite's native library (the same Java, built by GraalVM) and Python's
own DuckDB. So Python runs DataCube's non-screen code in an embedded JavaScript engine (V8, through `mini-racer`). It
plans through the native library and runs its queries on Python's DuckDB.

Measured: 1,123 of the 1,138 tests of DataCube's non-screen code pass in V8 as in Node. All 15 that do not pass need the
network (the fetch API) or are one test's own plumbing; none is DataCube misbehaving. There is one implementation of
DataCube's meaning, never copied into Python. It is reachable from scripts, CI and notebooks, without a screen.

## 1. What binds this design (the user's rulings, 2026-10-11)

- **The feature boundary is the relation.** Create a Class takes a relation, which is legend-lite's writer in core.
  DataCube's job is turning clicks into a relation. Python's job is turning code into one, or using a frame as it is.
- **DataCube's logic is DataCube's TypeScript, wherever it runs.** It is not copied into Python and not ported to Java.
  Python reuses it by running it.
- **Faithful to production.** The code under test runs the production paths. Test tooling may differ, but only in
  how it builds its inputs, never in what it tests. No recorded answers stand in for a computation.
- **The tests' synchronous fixture parses are a test habit, not a product need.** They become asynchronous as part of
  this design, not before (§6).
- **No WebAssembly in the Python process.**

## 2. What was measured (`docs/datacube-headless-evidence-2026-10-11/README.md`, in docs/HISTORY.md)

- **The query builder.** In V8 and QuickJS it is byte-identical to Node: on the WASM differential's 49 grids, on the
  same grids through the page-document path, and on 15,000 fuzzed builds.
- **DataCube's own tests, three engines.** In the 67 files of non-screen code: Node 1,138 passing, V8 1,123, QuickJS
  1,101; none passing outside Node that fails in it.
  - V8 loses 15: 14 need the fetch API (the remote planner's, the engine executor's and the warehouse sign-in's HTTP
    clients), and 1 is a test reading a DuckDB file synchronously.
  - QuickJS loses those and 23 more, for `Intl` (number and date formatting).
- **The engines.**
  - V8 (`mini-racer` 0.14, V8 14.4): wheels for all five targets, +15-22 MB, +51 MB of memory, `Intl` built in,
    about 7x faster than QuickJS on heavy JavaScript.
  - quickjs-ng: +0.5-2.6 MB, +7 MB; no `Intl`; no macOS x86_64 wheel; its binding one person's.
- **The choice: V8.** It formats as the browser does, it is fast, and it is packaged for every target. Its cost is
  size, and memory beside Python.

## 3. The shape

```
Python process
├── legend_lite (native library, GraalVM)   the compiler and planner: the same Java as the tab's WASM module
├── duckdb (Python)                          the database: the frames, the classes' tables
└── V8 (mini-racer), on its own thread       DataCube's non-screen bundle
      ├── WasmPlanner ── worker transport ──► native library      (asynchronous; the browser's production path)
      ├── DuckDbEngine ── connection ──────► Python's DuckDB      (asynchronous; Arrow IPC)
      └── the host's web layer               URL, TextEncoder/Decoder, AbortController, events, timers, crypto.subtle
```

- **The planner.** It is DataCube's `WasmPlanner`, unchanged, over its worker transport. The browser already runs its
  module in a Web Worker (`planner-worker.ts`); here the "worker" is the host, which answers each message with the
  native library's function of the same name (`lite_hold_model`, `lite_plan_json_held`, ...).
  - **Each request kind is answered by the same `Boundary` function in both hosts** (the Plan Gen line's review). The
    worker's nine kinds (`hold`, `release`, `relationType`, `tableModel`, `catalogColumnsSql`, `planJson`, `pureV1`,
    `testData`, `plan`) are one exported TypeScript type, which both hosts are checked against (§6).
  - Three had no native function, and have one beside the held functions in `native/.../nativelib/Compiler.java`
    (step 1c): `plan`, as `lite_plan_text_held` over `Boundary.planHeld` (the query planned against the model held,
    compiled once); `relationType`, as `lite_relation_type_text_held` over `Boundary.relationTypeHeld` (Pure text: the
    library's `lite_relation_type_held` reads a lambda's JSON); and `testData`, as `lite_test_data_sql`. The
    measurement answered `plan` by `lite_plan_text` over the held model's text, which compiles the whole model again
    for every query, and `relationType` by the JSON function: neither production's path. The design does neither.
  - `WasmPlanner` names how the browser delivers its module, not what it is. It is renamed `LitePlanner` (ruled, §10),
    with the Plan Gen / Exec Split line, whose boundary it is.
- **The database.** It is DataCube's `DuckDbEngine`, unchanged, over a connection whose `query(sql)` runs on Python's
  DuckDB and answers Arrow IPC, read by `apache-arrow`. DuckDB-WASM's file functions (`registerFileBuffer`, ...) are
  answered by writing into a folder of the host's own, which Python's DuckDB reads.
- **Big data does not cross into JavaScript.** A grid's view goes through DataCube's code as a grid shows it: levels
  capped, as on screen. A whole result goes another way. For `grid.to_pandas()` DataCube answers the query, and
  Python's DuckDB returns the Arrow table to Python directly.
- **The web layer** is a small module of the host's. The browser supplies these things and V8 alone does not:
  - `URL` and `URLSearchParams` (`whatwg-url`, the specification's reference implementation);
  - `TextEncoder` and `TextDecoder` (UTF-8);
  - `EventTarget`, `Event`, `DOMException`, `AbortController` and `AbortSignal`;
  - timers, on the host's own loop;
  - `crypto.subtle.digest`, by Python's `hashlib`.

  Each is held to its standard by a conformance test of its own (§6). The fetch API is left out, since nothing in
  Python needs DataCube's HTTP clients.

## 4. The headless API (what a developer types)

```python
page = ll.Page('Trades')
trades = page.grid(trades_df, name='trades').filter(...).calculate('big', 'x|$x.notional > 100').group('region')

trades.view()               # what the grid shows: its rows, levels, totals and truncation, as DataCube computes them
trades.relation             # its rows as a relation (Pure), at the finest level, as Create a Class makes it
trades.to_pandas()          # the relation's whole result, from Python's DuckDB
trades.create_class('Trade', package='trades')       # a class of the grid as DataCube means it, in the same cell
page.export('trades.xlsx')  # a page's export, as the screen writes it
```

- **One grid** is almost there. A grid's run loop is already screen-free: `CubeController.run(state)` produces its view
  (rows, tree, truncation, total rows), driven by the state owner (`state-owner.ts`); the screen (`app.ts`) wires them
  to the DOM. The headless grid assembles the two, behind an entry point of the bundle's.
- **A page** needs more. Its orchestration of several grids, their charts and sheets is in screen modules today
  (`page/page-app.ts`, `page/cube-page.ts`). The design's second step moves what is not drawing into a screen-free
  page engine, which the screen and Python then share. That is the largest piece of new work, and a good refactor in
  its own right.
- **`create_class` from a grid now works in code, in the same cell.** It uses DataCube's builder, so the grid's
  filters and calculated columns go into the class's function. This removes §15.5's limit, under which Python made
  classes from frames or written relations only. Those two ways stay too.

## 5. The asynchronous boundary, and notebooks

- `mini-racer` runs V8 on its own thread, and every call from JavaScript into Python is asynchronous (a Promise).
  DataCube's production paths are asynchronous already, so this costs nothing in them.
- Python's API is **synchronous** to the developer and safe in Jupyter. legend-lite owns one event loop on a
  background thread for the engine; a call such as `grid.view()` is posted to it and waited for. It never calls
  `asyncio.run()`, which fails inside Jupyter's own running loop. Asynchronous variants (`await grid.view_async()`) are
  offered for code already in a loop.
- **One engine per process, one call at a time** into DataCube's state, as on screen, where one state owner holds a
  grid's state. `mini-racer` serializes calls into one context (measured: one context from four threads, correct). Making
  contexts on several threads at once crashed it (measured), so the engine's context is made once, on its own thread.
  Its memory is capped (`set_hard_memory_limit`), so a runaway grid fails its call instead of the process.

## 6. Keeping it proven: the lane

The measurement becomes a test, `//datacube:headless_test`, run on all three platforms:

- DataCube's non-screen test files, run in V8 by Python, through the production paths of §3.
- **The test helpers made asynchronous**, landing for real: `datacube/test/lite-compiler.ts` and
  `pure-protocol/test/lite.ts`, as the overlay did (`overlay.patch`: their callers in 18 files, 232 awaits once a wrapper
  that awaits is awaited in turn, by the codemod and three hand fixes). The same tests then run unchanged in Node and in V8.
- **The host's hooks become supported options** instead of the overlay's patches:
  - the planner's worker factory: `WasmPlanner`'s option, not a global `Worker`;
  - the DuckDB connection the tests open;
  - the runfiles placeholder.
- **Conformance tests for the web layer**, each against its standard's own test vectors where they exist (`URL`:
  whatwg-url's; UTF-8: the Encoding standard's).
- **The worker contract's guard** (the real guard of §7's check): a lane test that the request kinds' type,
  `planner-worker.ts`'s switch and the Python host's table are the same set, so a new kind fails both hosts at once.
- **A guard:** the headless bundle imports nothing of the DOM or of Node (esbuild `--platform=neutral`, and the lane's
  run).
- **An opportunity, not a commitment:** with the lane, DataCube's unit tests no longer need Node to run (the "no Node in
  legend-lite" goal).

**In the build** (the Compiler Rewrite line's review, 2026-10-11):

- **Its lane is `//gates:warehouse`**, beside `//datacube:python_engine_test`, not `datacube` and not a lane of its own.
  The test loads the native library (`//native:compiler`), a whole-machine native-image build. The warehouse lane builds
  that image in a step of its own before its tests; the `datacube` lane is in `//gates:local` and does not build it.
  So the test is not in the local gate: it is run by name (`bazel test //datacube:headless_test`) before a push, as
  `//python`'s tests are. In CI it runs on all three platforms, and linux-arm runs the warehouse lane alone, so four
  of the five wheel targets are tested (macOS x86_64 has no runner, as for the wheel today).
- **`mini-racer==0.14.1`** in `tools/python/requirements.in`, with a dated reason, relocked by
  `bazel run //tools/python:requirements.update`. `MODULE.bazel.lock` moves with it, held by
  `//tools/python:lock_matches_requirements`. The test takes `deps = ["@pypi//mini_racer"]`, as `//python`'s take
  `@pypi//duckdb`: Bazel fetches the wheel by its lock hash and unpacks it at fetch time, so the test installs
  nothing, and V8 and its ICU data come inside the wheel. `target_compatible_with =
  compatible_with(COMPILER_LIBRARY_PLATFORMS)`, so Windows on Arm skips it rather than failing it.
- **The bundles:** one testonly `esbuild_bundle` (`--platform=neutral`), the test files as its entry points into one
  output directory (one cached action). The fetch-needing files are left out in BUILD by name, with the reason, so the
  exclusion is the build's, not the run's. `apache-arrow` comes through `//engine-client:engine_client`. The `node:stream`
  stub is a file of the package named as an esbuild alias, and the no-Node guard knows of it. `whatwg-url` goes into
  `datacube/package.json`, relocked through Bazel's pnpm and held by `//tools/js:lock_matches_package_json_test`.
- **The test:** a `py_test`, `size = "medium"`; deps `//python:compiler`, `@pypi//mini_racer`, `@pypi//duckdb`,
  `@pypi//pyarrow`, `@rules_python//python/runfiles`; data `//native:compiler` and the bundles; env as
  `python/BUILD.bazel`'s `_ENV`; its DuckDB files under `TEST_TMPDIR`. No path, program or PATH lookup.
- **Windows:** the native library and duckdb's extension load under Bazel on Windows already. The first Windows run
  checks that `py_mini_racer` loads its `mini_racer.dll` (with `icudtl.dat` and the snapshot beside it, and the MSVC
  runtime, which windows-2022 has). If it does not, the fix is how the DLL is loaded, never a host install.

## 7. Packaging

- **One package with an extra: `pip install legend-lite[datacube]`** (ruled, §10). The extra
  brings `mini-racer` (V8), and the wheel carries DataCube's headless bundle (small), built by Bazel's esbuild.
- Two wheels (`legend-lite` and `legend-lite-datacube`) would keep the same parts apart. But the bundle and the native
  library must agree on their interface exactly, so two wheels would need a pinned pair and a version handshake. One
  package has one version.
- The existing `notebook` extra is the same pattern. `//python:wheel_test`'s offline install from Bazel's downloads
  gains `mini_racer` in its package lists, per platform, as the notebook's lists are.
- **The extra raises the glibc floor for its users only:** `mini-racer`'s Linux wheels are `manylinux_2_27`, and
  legend-lite's wheel says `2_17`. The extra's documentation says so.
- **An interface check at start, by lists, not a version** (the Plan Gen line's review: `Boundary` has no version, and a
  hand-kept one gets forgotten). The bundle lists the request kinds it sends (`REQUEST_KINDS`); the host's one table
  of kind to native function (`KINDS`, `legend_lite/_headless.py`) is checked against it, and each function against
  the library's own export table (`Library.exports`: the linker's list, none kept by hand; agreed with the Plan Gen
  line in place of a `lite_interface()`). A mismatch fails at start, with both sides named.

## 8. What it costs, and the risks

- **The page engine (§4)** is the largest new piece: the screen's orchestration moved into screen-free modules.
- **Three runtimes in one process:** Python, GraalVM's native library and V8. That means +51 MB, start-up of the order
  of a hundred milliseconds (to be measured), and errors that cross three languages. A V8 out-of-memory failure kills
  the process unless capped (§5).
- **The same DataCube is not the same database.** The browser's DuckDB-WASM and Python's DuckDB are different
  releases: an edge-case type or an error's words can differ. `Intl` follows the ICU data bundled with `mini-racer`'s
  V8. It matched Node's in all 23 measured formatting tests, and may differ from a user's Chrome in an edge case.
- **A dependency to own:** `mini-racer`, its V8 upgrades and security releases at its pace. The JavaScript run is ours,
  never a user's, which lowers the exposure.
- **Two worker kinds have no native function** (`plan` and `testData`, §3). They are added to the native library beside
  its other held functions, so the headless planner answers what the browser's does, by the same Java.
- **Size:** +15-22 MB for the extra's users.

## 9. The plan (in order; each a landing of its own)

0. **The rename** (§3): `WasmPlanner` to `LitePlanner`, `wasm-planner.ts` to `lite-planner.ts`, with its test, its
   callers and the comments that name it (in `wasm/`'s `Boundary.java` and `README.md`, and `tools/teavm/TeaVmCompile.java`,
   comment-only, told to the Compiler Rewrite line). A landing of its own, so the lane's diff is only the lane. The
   name agreed by the Plan Gen / Exec Split line.
1. **The lane (§6).**
   - The test helpers made asynchronous; the hooks made supported options; the web layer with its conformance tests;
     the worker's request kinds as one type, with their guard; `lite_plan_text_held`, `lite_relation_type_text_held`
     and `lite_test_data_sql` natively (entry points in `native/` over `Boundary`'s existing functions). Built (its record, §11, cut).
   - Reviews: the Compiler Rewrite line for the BUILD diff and the new pinned Python dependency (`mini-racer` in
     `@pypi`); the Plan Gen / Exec Split line for the planner's hooks, the worker contract and the native entry points.
   - Before the push: the lane query (`tests(//...) except tests(//gates:lanes) except tests(//gates:heavy)` empty),
     `bazel test //datacube:headless_test` by name, and CI on the branch with `-f gates=warehouse,checks,datacube -f
     platforms=all`. The lock change rolls CI's downloads cache key, so that first run downloads everything again,
     once.
2. **The headless grid** (§4: the bundle's entry point; `grid.view()`, `grid.relation`, `grid.to_pandas()`), and the
   engine's loop thread (§5).
3. **The page engine** (§4): the page's orchestration moved out of the screen, shared by the screen and Python; then
   `page.export(...)`.
4. **Packaging** (§7): the `datacube` extra, the interface check, the wheels on all five targets.
5. **The model plan's Python step** (§15.5) rides on it: `grid.create_class`, `create_association` and
   `create_data_space`, from grids, relations and frames.

**Where it goes relative to the model plan** (ruled, §10): before §15.5's Python calls, since `grid.create_class`
depends on steps 1-2. Steps 3-4 can follow the model plan's step 3 (dashboards on classes).

## 10. Ruled (the user, 2026-10-11)

1. **One package**, with a `datacube` extra, not two wheels. *Extended 2026-10-15 (the user: "core is just the compiler
   and datacube installs everything datacube needs including v8"): the wheel alone is the compiler and requires
   nothing; the extra is all of DataCube -- duckdb, pyarrow and mini-racer -- and every DataCube module asks for it
   first (`legend_lite/_extra.py`), so DataCube's code can always be asked and its rules are never copied into Python
   (`docs/DATACUBE_ONE_APP_DESIGN_2026_10_11.md` §16).*
2. **V8** (`mini-racer`), at its size (+15-22 MB, +51 MB of memory), not QuickJS.
3. **Before the model plan's Python calls** (§15.5): this design's steps 0-2 first.
4. **The rename**, `WasmPlanner` to `LitePlanner`, with the Plan Gen / Exec Split line (agreed by it, 2026-10-11).

> Cut in the 2026-11 docs cleanup: §11 to §14, all 2026-10-11 on branch `datacube-headless` -- step 1 as built (its six commits), steps 2 and 4 as built, every non-screen test run headless (7d973ea9e), and the branch's audit (2 blockers, 9 should-fix, each answered), with two limits said rather than built (a connection given to `Headless` keeps its search path; Python's host answers a query whole, so a superseded query is not cancelled between batches as in the tab). Full text: `git show de2745351:docs/DATACUBE_HEADLESS_PYTHON_DESIGN_2026_10_11.md`.

