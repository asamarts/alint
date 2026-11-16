#!/usr/bin/env bash
# Run the workspace's tests with every YAML decode checked against libyaml and
# saphyr (ADR-0041): alint-core's `yaml-reference` feature, with
# ALINT_YAML_PARSER=compare. A decode returns ayaml's result, and one whose
# difference neither saphyr's reading nor the differential harness's allowlist
# explains panics in the test that made it. First, check that no release build
# holds a reference parser.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

bash ci/scripts/yaml-reference-gate.sh

echo "==> Running cargo test --workspace --no-fail-fast with ALINT_YAML_PARSER=compare"
ALINT_YAML_PARSER=compare \
  cargo test --workspace --locked --no-fail-fast --features alint-core/yaml-reference
