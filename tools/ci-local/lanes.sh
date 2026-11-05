#!/usr/bin/env bash
# ONE PLATFORM'S GATES, LOCALLY: the steps .github/workflows/gates-run.yml runs per lane, in order, with its flags
# (tools/ci/lane-flags.sh, sourced as CI sources it). Run from the checkout's root, natively (macOS) or inside the
# container (tools/ci-local/ci.sh). Reads PLATFORM (linux, linux-arm, macos), LOW_MEMORY (true: --config=ci-small)
# and LINUX_ONLY_TESTS (ci-policy) as gates-run.yml's steps do, OUT (where each lane's log and the summary go), and
# three of its own: EXTRA_FLAGS (Bazel flags added to every command), CPUS and SKIP_TESTS (both below).
# Arguments: the lanes to run (default: every lane CI runs on PLATFORM -- linux-arm runs the warehouse alone).
#
# Two differences from CI, both only where outputs go: the checkout may be read-only (the container mounts it so), so
# Bazel writes no convenience symlinks (--symlink_prefix=/) and the logs are copied out by `bazel info`; and builds
# may share a local disk cache (EXTRA_FLAGS), where CI shares BuildBuddy. Test results are never cached (--config=ci).
set -uo pipefail

: "${PLATFORM:?PLATFORM is linux, linux-arm or macos}"
: "${LOW_MEMORY:=false}"
: "${LINUX_ONLY_TESTS:=ci-policy}"
: "${OUT:?OUT is the directory the logs go to}"
export LOW_MEMORY PLATFORM LINUX_ONLY_TESTS
mkdir -p "$OUT"

ALL_LANES=(product core checks corpus_duckdb corpus_h2 pct_duckdb pct_h2 pct_postgres pct_channel_b
  parser_equivalence stress warehouse datacube ui sdlc)
if [ "$#" -gt 0 ]; then
  lanes=("$@")
elif [ "$PLATFORM" = linux-arm ]; then
  lanes=(warehouse)
else
  lanes=("${ALL_LANES[@]}")
fi

# the lane's flags, exactly as CI defines them (flags, qflags, tflags)
# (checked on its own: //tools/guards:ci_scripts_test lints it too)
# shellcheck disable=SC1091
. tools/ci/lane-flags.sh
# (each optional list spelled ${a[@]+"${a[@]}"}: macOS's bash 3.2 calls an empty array unbound under set -u)
read -r -a extra <<< "${EXTRA_FLAGS:-}"
# CPUS (set by ci.sh in the container): the CPUs Bazel may use, said outright rather than left to its detection
if [ -n "${CPUS:-}" ]; then extra+=("--local_resources=cpu=$CPUS"); fi
flags+=(--symlink_prefix=/ ${extra[@]+"${extra[@]}"})
qflags+=(${extra[@]+"${extra[@]}"})
tflags+=(--symlink_prefix=/ ${extra[@]+"${extra[@]}"})
# SKIP_TESTS: tests a lane leaves out here, never on CI -- the ones this machine cannot judge (README.md's environmental
# failures), so that the lane's later steps still run and its status speaks for the rest. Subtracted as negative
# target patterns, after the lane's test_suite expands.
skip=()
for t in ${SKIP_TESTS:-}; do skip+=("-$t"); done

summary="$OUT/summary.tsv"
printf 'lane\tstatus\tseconds\n' > "$summary"

run_lane() {
  local lane=$1
  case "$lane" in
    product)
      bazel build "${flags[@]}" --skip_incompatible_explicit_targets //:java //:web //:wasm //:native //:sites \
        //datacube:app //datacube:app_package //native:compiler || return 1
      bazel build --nobuild --config=bazel10 "${flags[@]}" //... //:update_generated //tools/census:render_census \
        //tools/census:lanes_diff //tools/junit:compare_testcases //wasm:startup || return 1
      local S='//warehouse/... + //pct/... + //datacube/...' list unlisted=() unlaned
      list=$(bazel query "${qflags[@]}" "($S) except tests($S) except kind(test_suite, $S)" | grep -v '\.aspect_rules_js') \
        || return 1
      [ -n "$list" ] || { echo "A25: the query returned no targets"; return 1; }
      while IFS= read -r t; do unlisted+=("$t"); done <<< "$list"
      bazel build --nobuild --skip_incompatible_explicit_targets --platforms=//tools/platforms:unlisted_test \
        "${flags[@]}" "${unlisted[@]}" || return 1
      unlaned=$(bazel query "${qflags[@]}" 'tests(//...) except tests(//gates:lanes) except tests(//gates:heavy)') \
        || return 1
      [ -z "$unlaned" ] || { echo "tests in no lane of gates/BUILD.bazel:"; echo "$unlaned"; return 1; }
      ;;
    warehouse)
      bazel build "${tflags[@]}" --skip_incompatible_explicit_targets //native:compiler || return 1
      bazel build "${tflags[@]}" --build_tests_only -- //gates:warehouse ${skip[@]+"${skip[@]}"} || return 1
      bazel test "${tflags[@]}" -- //gates:warehouse ${skip[@]+"${skip[@]}"} || return 1
      local said code
      said=$(bazel run "${flags[@]}" //warehouse:serve -- --port 'x&y z' 2>&1)
      code=$?
      printf '%s\n' "$said" | tail -2
      [ "$code" = 2 ] || { echo "bazel run //warehouse:serve exited $code, not 2 (the refusal)"; return 1; }
      printf '%s\n' "$said" | grep -F "takes a number, not 'x&y z'" > /dev/null \
        || { echo "the argument did not reach the server intact"; return 1; }
      ;;
    *)
      bazel test "${tflags[@]}" -- "//gates:$lane" ${skip[@]+"${skip[@]}"} || return 1
      ;;
  esac
}

failed=0
for lane in "${lanes[@]}"; do
  echo "=== lane $lane ($PLATFORM) ==="
  start=$(date +%s)
  if run_lane "$lane" > "$OUT/$lane.log" 2>&1; then status=PASSED; else status=FAILED; failed=1; fi
  end=$(date +%s)
  printf '%s\t%s\t%s\n' "$lane" "$status" "$((end - start))" >> "$summary"
  echo "--- $lane: $status in $((end - start))s"
  # the failing tests' logs, as CI's artifact keeps them
  if [ "$status" = FAILED ]; then
    testlogs=$(bazel info "${flags[@]}" bazel-testlogs 2>/dev/null)
    if [ -n "$testlogs" ] && [ -d "$testlogs" ]; then
      grep -E '^(FAIL|TIMEOUT): //' "$OUT/$lane.log" | sed -E 's/^[A-Z]+: (\/\/[^ ]+).*/\1/' | while read -r t; do
        p=${t#//}; p=${p/://}
        mkdir -p "$OUT/testlogs/$p"
        if [ -f "$testlogs/$p/test.log" ]; then cp "$testlogs/$p/test.log" "$OUT/testlogs/$p/"; fi
      done
    fi
  fi
done
cat "$summary"
exit "$failed"
