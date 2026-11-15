#!/usr/bin/env bash
# A release build holds no reference YAML parser (ADR-0041): with default
# features, on any target, no workspace crate depends on `yaml_serde` outside
# its dev-dependencies. Only alint-core's `yaml-reference` feature, for testing,
# brings it in, and the control below proves the query finds it then, so an
# empty answer is not a query that matches nothing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

query=(cargo tree --locked --workspace --target all -e normal,build -i yaml_serde)

echo "==> Checking that a release build depends on no reference YAML parser"
found="$("${query[@]}" 2>/dev/null)"
if [[ -n "$found" ]]; then
  echo "error: a default build depends on yaml_serde, a test-only reference parser (ADR-0041):" >&2
  echo "$found" >&2
  exit 1
fi

control="$("${query[@]}" --features alint-core/yaml-reference 2>/dev/null)"
if [[ "$control" != *"alint-core"* ]]; then
  echo "error: with alint-core's yaml-reference feature, the query did not find yaml_serde;" >&2
  echo "       it would pass a release build that holds it" >&2
  exit 1
fi
echo "    no default build depends on yaml_serde"
