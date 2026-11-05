# CI, locally

The gate lanes of `.github/workflows/gates-run.yml`, run on this machine instead of a CI run: the same steps per lane,
in the same order, with the same flags (`tools/ci/lane-flags.sh`, sourced as CI sources it).

```
tools/ci-local/ci.sh macos [lane...]          # natively
tools/ci-local/ci.sh linux-arm64 [lane...]    # CI's linux-arm job (ubuntu-24.04-arm), natively on Apple silicon
tools/ci-local/ci.sh linux-amd64 [lane...]    # CI's linux job (ubuntu-latest), under Docker Desktop's emulation
```

No lane given runs every lane CI runs on that platform (CI's linux-arm job runs the warehouse alone; name lanes to run
more). The lanes: `product core checks corpus_duckdb corpus_h2 pct_duckdb pct_h2 pct_postgres pct_channel_b
parser_equivalence stress warehouse datacube ui sdlc`. Each lane's log, the failing tests' logs and `summary.tsv` go to
`$OUT` (default `~/.cache/legend-lite-ci/<platform>/<time>`); the script exits non-zero when a lane fails.

- **Linux** runs in a container of CI's runner (`Dockerfile`: Ubuntu 24.04, the host prerequisites gates-run.yml
  installs, the pinned bazelisk, a non-root user). The checkout is mounted read-only at its own path, with the
  repository a worktree belongs to; each platform's Bazel caches persist in a Docker volume
  (`legend-lite-ci-cache-<platform>`). The container gets CI's runner's 4 CPUs (`CPUS`, default 4: Docker's `--cpus`
  and Bazel's `--local_resources=cpu`), so as many tests run at once as on CI -- given the VM's every CPU in its
  smaller memory, the datacube lane's browsers crashed ("Target crashed") and passed one at a time (2026-11-04).
  `LOW_MEMORY=true` runs one test at a time (`--config=ci-small`, as CI's 7 GB macOS runner does).
- **Not emulated**: Windows (no Windows host here), and BuildBuddy -- a local `--disk_cache` (via `EXTRA_FLAGS`) stands
  in for the remote cache; test results are never cached (`--config=ci`).
- Bazel writes no convenience symlinks (`--symlink_prefix=/`): the logs are copied out through `bazel info`.

## Known environmental failures (not the branch's)

- **The warehouse's GraalVM native image segfaults under Docker Desktop** (its handler prints `Segfault detected,
  aborting process`; the server exits 99 before listening). It fails every test that starts `//warehouse:server_native`
  on Linux in the container: `//warehouse:tests_native` and `postgres_live_native`, `//datacube:python_engine_test`,
  `live_snap_test` and `verify_app_test` (the app carries the image), and `//query:verify_test`'s warehouse section
  (its browser and server sections pass). Measured 2026-11-04 on `cf73e6ef7`, before the Rust port, in the same image:
  `tests_native` and `python_engine_test` fail the same way there, so read these on CI's hosted runners (or on Linux
  hardware), not here. The compiler library (`//native:compiler`) runs in the container.
- **`linux-amd64` under Rosetta is slow, and its tests time out four at a time.** The JVM's JIT writes x86 code that
  Rosetta must translate again as it runs: on 2026-11-05 (4 CPUs, an empty cache) the core lane took 9.7 hours and 19
  of its 64 tests timed out (15 of its 29 JUnit suites, the three Cypher parities, the MIR round trip), with no
  assertion failure in any log; `//datacube:jupyterlab_test` timed out waiting for its kernel; the native image
  segfaults as on arm64. Run alone with a long timeout, the two tried (`//core:core_tests_sql`,
  `//rust/lite-sql:mir_round_trip_test`) passed in minutes. For a verdict there, run one test at a time:
  `LOW_MEMORY=true EXTRA_FLAGS='--test_timeout=14400 ...'`. What is x86-64's own -- the Rust crates' tests but the
  MIR round trip, the FFI shim (`//core:ffi_test`), the dialect differential, the wheel's tag (`//python:wheel_test`),
  the Python bindings -- and the checks and product lanes passed four at a time.
- **Docker Desktop's VM memory, if below CI's 16 GB**: the browser tests crash ("Target crashed") or are killed
  without a word when too many run at once. At 4 CPUs in a 10 GB VM, three datacube tests did (`join_open_test`,
  `timeseries_queries_test`, `verify_flow_test`), and each passed run alone (`--config=ci-small`). Give the VM 16 GB
  or more, or rerun such a test alone before calling it red.

Leave them out with `SKIP_TESTS`, so a lane's later steps still run (the warehouse lane's `bazel run` check comes after
its tests) and its status speaks for the rest:

```
SKIP_TESTS='//warehouse:tests_native //warehouse:postgres_live_native //datacube:python_engine_test
  //datacube:live_snap_test //datacube:verify_app_test' tools/ci-local/ci.sh linux-arm64 warehouse datacube
```

Each is subtracted from the lane as a negative target pattern (`bazel test -- //gates:<lane> -<test>`); the lane's log
says how many tests ran. CI never skips them.
