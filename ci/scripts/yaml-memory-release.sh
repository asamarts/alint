#!/usr/bin/env bash
# The release condition of ADR-0042: decoding a YAML document's JSON tree,
# ayaml's peak heap is within 1.2x of libyaml's (`yaml_serde` 0.10.7, the parser
# v0.22 used), measured by alint-core's `yaml_memory` test on generated OpenAPI
# documents of about 37 KB and 1.1 MB. v0.23.0 waits for an ayaml release that
# meets it.
#
# The release preflight runs it. CI's `YAML Compare` job runs the same test
# (yaml-compare.sh) against the regression bound, today's ratio and about 10%.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

echo "==> Checking that ayaml's peak heap is within 1.2x of libyaml's (ADR-0042)"
ALINT_YAML_MEMORY_RELEASE=1 \
  cargo test -p alint-core --locked --features yaml-reference --test yaml_memory -- --nocapture
