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

# `xtask gen-facts --check` regenerates facts.json (the surface-area contract:
# version + the six headline counts + catalogue lists) from the same canonical
# sources coverage_audit_readme_claims pins the README to, and fails if the
# committed facts.json drifted. Phase 3 / WS1e of the spec-driven program
# (docs/design/facts-json.md). Run `cargo run -p xtask -- gen-facts` to refresh
# after adding a rule kind, family, ruleset, fixer, formatter, or subcommand.
echo "==> Running xtask gen-facts --check"
cargo run -q -p xtask -- gen-facts --check

# `xtask gen-roadmap --check` regenerates roadmap.json (the public-roadmap
# contract the alint.org /roadmap/ timeline renders) from the marked phase
# headings in docs/design/ROADMAP.md, and fails if the committed roadmap.json
# drifted. Run `cargo run -p xtask -- gen-roadmap` to refresh after editing a
# phase heading or its roadmap-public marker.
echo "==> Running xtask gen-roadmap --check"
cargo run -q -p xtask -- gen-roadmap --check

# `xtask gen-arch --check` regenerates the crate dependency graph
# (docs/design/architecture/crate-graph.md) from `cargo metadata` and fails if
# it drifted, and verifies the hand-modeled C4 model (workspace.dsl) still
# declares exactly the workspace crate set. Phase 4 / WS3 of the spec-driven
# program (docs/design/architecture-as-code.md). Run `cargo run -p xtask --
# gen-arch` to refresh after adding/removing a crate or an intra-workspace dep.
echo "==> Running xtask gen-arch --check"
cargo run -q -p xtask -- gen-arch --check

# `xtask gen-model --check` regenerates the code-derived LikeC4 architecture-model
# fragments (docs/design/architecture/model/*.gen.c4 — currently the rule-kind
# taxonomy sliced from docs/rules.md) and fails if a committed fragment drifted
# from its canonical source. The merged LikeC4 model's structural validity
# (`likec4 validate`) is gated separately; see
# docs/design/architecture-diagrams.md. Run `cargo run -p xtask -- gen-model` to
# refresh after changing a source (e.g. adding a rule family or kind).
echo "==> Running xtask gen-model --check"
cargo run -q -p xtask -- gen-model --check

# Validate the LikeC4 architecture model itself: `likec4 validate` proves every
# dynamic-view (flow) step references a real, declared element/relationship -
# the structural-integrity gate for the hand-authored flows. Separate script
# because it needs Node; it loudly skips if Node is absent until the runner
# image ships it. See ci/scripts/likec4.sh + docs/design/architecture-diagrams.md.
echo "==> Running ci/scripts/likec4.sh"
bash ci/scripts/likec4.sh
