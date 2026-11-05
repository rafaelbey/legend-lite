# CI's Linux runner, locally (tools/ci-local/README.md): Ubuntu 24.04, as ubuntu-latest and ubuntu-24.04-arm are, with
# what .github/workflows/gates-run.yml adds to them -- libxml2 (the hermetic LLVM's ld.lld links it) and the pinned
# bazelisk, checked by its sha256 -- and what the hosted image already carries that the build reaches: a C compiler,
# git, python3, zip, and the shared libraries the hosted image's own browsers bring, which the Chromium headless shell
# Bazel fetches loads (the datacube lane's browser tests). Everything else (the JDKs, GraalVM, Node, Chromium, Postgres,
# the Rust toolchain) Bazel fetches itself, as on CI. Built for linux/arm64 natively, linux/amd64 under Docker
# Desktop's emulation.
FROM ubuntu:24.04

RUN apt-get update -qq \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
        build-essential ca-certificates clang curl file git jq libxml2 lld locales python3 sudo tzdata unzip zip \
        fonts-liberation libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 libatspi2.0-0t64 libcairo2 libcups2t64 \
        libdbus-1-3 libdrm2 libgbm1 libglib2.0-0t64 libnspr4 libnss3 libpango-1.0-0 libx11-6 libxcb1 \
        libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2 \
    && rm -rf /var/lib/apt/lists/*

# the bazelisk gates-run.yml installs where the image has none, by the same sha256
RUN set -eu; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
      amd64) sha=5a408715e932c0250d28bd84555f12edbf70117de42f9181691c736eacc4a992 ;; \
      arm64) sha=e20e8b0f4f240091b7a55bf17b9398bd4f40ee70ae0208dff95dd4c445fb4010 ;; \
      *) echo "no bazelisk for $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /usr/local/bin/bazel "https://github.com/bazelbuild/bazelisk/releases/download/v1.29.0/bazelisk-linux-$arch"; \
    echo "$sha  /usr/local/bin/bazel" | sha256sum -c -; \
    chmod +x /usr/local/bin/bazel

# the runner's user: not root (an embedded Postgres refuses root), with passwordless sudo as the hosted runner has
RUN useradd -m -u 1001 -s /bin/bash runner \
    && echo 'runner ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/runner \
    && locale-gen en_US.UTF-8
ENV LANG=en_US.UTF-8 TZ=UTC
USER runner
WORKDIR /home/runner
