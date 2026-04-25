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
