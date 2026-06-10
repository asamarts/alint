#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running cargo doc"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

# `xtask docs-export --check` writes the bundle to a tempdir and
# discards it. It re-parses every bundled ruleset YAML, captures
# `alint --help` per subcommand, and exits non-zero on any failure
# — so a stale rule manifest, a broken ruleset YAML, or a CLI that
# refuses --help fails CI rather than the alint.org build.
echo "==> Running xtask docs-export --check"
cargo run -q -p xtask -- docs-export --check

# `xtask gen-schema --check` regenerates schemas/v1/config.json (+ the in-crate
# copy) from the Rust option structs (schemars) for the migrated rule kinds and
# fails if the committed schema drifted from the types that parse configs. The
# regenerate-and-diff gate for the schema-from-types keystone (ADR-0001 /
# docs/design/spec-driven-development.md). Run `cargo run -p xtask -- gen-schema`
# to refresh after changing a migrated kind's Options struct.
echo "==> Running xtask gen-schema --check"
cargo run -q -p xtask -- gen-schema --check
