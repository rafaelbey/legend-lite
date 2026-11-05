#!/usr/bin/env bash
# CI, LOCALLY (tools/ci-local/README.md): one platform's gate lanes as .github/workflows/gates-run.yml runs them --
# macOS natively, Linux in a container of CI's runner image (tools/ci-local/Dockerfile) -- so a branch is judged
# before a push, without a CI run.
#
#   tools/ci-local/ci.sh macos [lane...]
#   tools/ci-local/ci.sh linux-arm64 [lane...]     # natively on Apple silicon; CI's linux-arm job runs the warehouse
#   tools/ci-local/ci.sh linux-amd64 [lane...]     # CI's linux job (ubuntu-latest), under Docker Desktop's emulation
#
# No lane given: every lane CI runs on that platform. Logs and summary.tsv go to $OUT (default
# ~/.cache/legend-lite-ci/<platform>/<time>). LOW_MEMORY=true adds --config=ci-small (one test at a time), as CI's
# 7 GB macOS runner does; the default is CI's own (true on macOS, false on Linux). EXTRA_FLAGS adds Bazel flags (a
# --disk_cache, say); SKIP_TESTS leaves tests out of the lanes (README.md: the ones this machine cannot judge). The
# checkout is mounted read-only at its own path, with the repository it belongs to (a worktree's .git points there);
# each Linux platform keeps its Bazel caches in a Docker volume of its own.
set -euo pipefail

target=${1:?"usage: tools/ci-local/ci.sh macos|linux-arm64|linux-amd64 [lane...]"}
shift
here=$(cd "$(dirname "$0")" && pwd)
checkout=$(cd "$here/../.." && pwd)
stamp=$(date +%Y%m%d-%H%M%S)

case "$target" in
  macos)
    out=${OUT:-$HOME/.cache/legend-lite-ci/macos/$stamp}
    cd "$checkout"
    PLATFORM=macos LOW_MEMORY=${LOW_MEMORY:-true} OUT="$out" exec "$here/lanes.sh" "$@"
    ;;
  linux-arm64) docker_platform=linux/arm64; ci_platform=${CI_PLATFORM:-linux-arm} ;;
  linux-amd64) docker_platform=linux/amd64; ci_platform=linux ;;
  *) echo "unknown platform '$target': macos, linux-arm64 or linux-amd64" >&2; exit 2 ;;
esac

image="legend-lite-ci:${target}"
docker build --platform "$docker_platform" -t "$image" -f "$here/Dockerfile" "$here"

# the repository the checkout belongs to: a worktree's .git file names it, and git inside the container must find it
repo=$(git -C "$checkout" rev-parse --path-format=absolute --git-common-dir)
repo=$(dirname "$repo")
out=${OUT:-$HOME/.cache/legend-lite-ci/$target/$stamp}
mkdir -p "$out"
volume="legend-lite-ci-cache-${target}"

# CI's Linux runners have 4 CPUs (and 16 GB): as many here, so as many tests run at once -- the VM's every CPU in its
# smaller memory runs a browser per CPU and crashes them (the datacube lane, measured 2026-11-04)
cpus=${CPUS:-4}
docker run --rm --platform "$docker_platform" \
  --user runner \
  --cpus "$cpus" \
  -e CPUS="$cpus" \
  -v "$repo:$repo:ro" \
  -v "$checkout:$checkout:ro" \
  -v "$volume:/home/runner/.cache" \
  -v "$out:/out" \
  -w "$checkout" \
  -e PLATFORM="$ci_platform" \
  -e LOW_MEMORY="${LOW_MEMORY:-false}" \
  -e LINUX_ONLY_TESTS="${LINUX_ONLY_TESTS:-ci-policy}" \
  -e EXTRA_FLAGS="${EXTRA_FLAGS:-}" \
  -e SKIP_TESTS="${SKIP_TESTS:-}" \
  -e OUT=/out \
  "$image" bash -c 'sudo chown runner /home/runner/.cache && exec tools/ci-local/lanes.sh "$@"' lanes "$@"
