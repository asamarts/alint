# syntax=docker/dockerfile:1.7
#
# alint-bench — reproducible competitive-benchmark image.
#
# This image is purpose-built for `xtask bench-scale` to run
# inside a known-pinned environment so cross-machine results are
# directly comparable. Every benchmarked tool's version is pinned
# at build time; a given image tag (e.g. `0.5.7`) is therefore
# the canonical "bench environment for v0.5.7." Bumping any
# tool requires re-publishing the image and re-running the
# competitive numbers.
#
# Built + pushed to `ghcr.io/asamarts/alint-bench:<tag>` by
# `.github/workflows/bench-docker.yml` on tag pushes and on
# manual dispatch.
#
# Usage (typical):
#
#   xtask bench-scale --docker --tools all --sizes 1k,10k \
#                     --out ./bench-results
#
# The xtask `--docker` flag is a thin wrapper that runs:
#
#   docker run --rm -v "$PWD:/work" \
#     ghcr.io/asamarts/alint-bench:<tag> --tools all ...
#
# Direct invocation works too — the entrypoint forwards all
# args to `xtask bench-scale`.

# ─── Pinned tool versions ──────────────────────────────────────────────
# Bumping any of these is a meaningful change to the bench
# environment; bump the image tag and re-run results when you
# touch them.
#
# Hyperfine 1.20 is the first release whose `--ignore-failure=<codes>`
# form can distinguish lint findings from setup/internal errors. It requires
# Rust 1.88, so this purpose-built benchmark image pins a bench-only compiler
# independently of alint's user-facing MSRV. The actual compiler remains part
# of every result fingerprint.
ARG HYPERFINE_VERSION=1.20.0
ARG RIPGREP_VERSION=15.1.0
ARG REPOLINTER_VERSION=0.11.2
ARG LS_LINT_VERSION=2.2.3
ARG NODE_VERSION=20.20.2
ARG RUST_VERSION=1.88.0

FROM rust:${RUST_VERSION}-bookworm

ARG HYPERFINE_VERSION
ARG RIPGREP_VERSION
ARG REPOLINTER_VERSION
ARG LS_LINT_VERSION
ARG NODE_VERSION
ARG RUST_VERSION

# The mounted checkout requests `stable`, which moves over time. Override it
# with the exact compiler baked into this image so rebuilding/rerunning a tag
# cannot silently change the benchmark toolchain.
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION}

# Findutils for `find`; ca-certificates + curl for the Node
# nodesource installer; git for `git init` inside `--changed`
# mode; build-essential is already in `rust:` base. `valgrind` powers
# the deterministic (load-immune) perf benches — its version is pinned
# by the `rust:${RUST_VERSION}-bookworm` base (Debian bookworm ships
# valgrind 3.19), so the cachegrind cache/branch model is reproducible
# across image rebuilds. See docs/design/deterministic-perf-gating.md.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        findutils \
        git \
        valgrind \
    && rm -rf /var/lib/apt/lists/*

# Exact Node 20 runtime via NodeSource. Repolinter's runtime is part of the
# benchmark environment too, so pin the package instead of following the
# moving `20.x` repository head.
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends "nodejs=${NODE_VERSION}-1nodesource1" \
    && rm -rf /var/lib/apt/lists/*

# Bench harness tools — Rust binaries via cargo. `--locked`
# ensures the exact upstream Cargo.lock is used so the image
# is byte-reproducible across rebuilds for the same versions.
RUN cargo install hyperfine --version "${HYPERFINE_VERSION}" --locked \
    && cargo install ripgrep --version "${RIPGREP_VERSION}" --locked

# `xtask bench-scale --docker` runs as the host UID/GID so published result
# files are not root-owned. Cargo must still be able to populate its registry
# cache for the bind-mounted checkout.
RUN chmod -R a+rwX /usr/local/cargo

# Competitor tools — Node packages via npm.
# - Repolinter: pinned to the last pre-archive release.
# - ls-lint: official npm distribution since v2.
RUN npm install -g \
        "repolinter@${REPOLINTER_VERSION}" \
        "@ls-lint/ls-lint@${LS_LINT_VERSION}"

# Workspace mount point. The xtask wrapper bind-mounts the
# host repo at /work; the cargo target dir is on a separate
# named volume so the host's target/ doesn't collide with the
# container's build cache.
WORKDIR /work
ENV CARGO_TARGET_DIR=/cargo-target
RUN mkdir -p /cargo-target && chmod 1777 /cargo-target
VOLUME ["/cargo-target"]

# Tag the image so consumers can detect they're inside the
# bench env (e.g. for skipping interactive prompts).
ENV ALINT_BENCH_DOCKER=1

# Default entrypoint = the bench harness. Args after the image
# name flow into `xtask bench-scale ...` so a user can run:
#   docker run alint-bench:0.5.7 --sizes 1k --tools alint
ENTRYPOINT ["cargo", "run", "-p", "xtask", "--release", "--quiet", "--", "bench-scale"]
CMD []
