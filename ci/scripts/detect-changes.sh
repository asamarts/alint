#!/usr/bin/env bash
set -euo pipefail

# Detect which components changed to enable conditional CI pipelines.
# Outputs: rust=true/false, docs=true/false, bench=true/false,
#          examples=true/false, editors=true/false
#
# Environment variables (set by the workflow):
#   GH_EVENT         - github.event_name (push | pull_request)
#   PR_BASE_SHA      - github.event.pull_request.base.sha
#   PUSH_BEFORE_SHA  - github.event.before

# ── Determine base commit ────────────────────────────────────────────

if [[ "${GH_EVENT:-}" == "pull_request" ]]; then
  BASE="${PR_BASE_SHA:?PR_BASE_SHA required for pull_request events}"
elif [[ -n "${PUSH_BEFORE_SHA:-}" \
     && "${PUSH_BEFORE_SHA:-}" != "0000000000000000000000000000000000000000" ]]; then
  # Normal push — verify the before SHA still exists (force-push may rewrite).
  if git cat-file -e "${PUSH_BEFORE_SHA}^{commit}" 2>/dev/null; then
    BASE="$PUSH_BEFORE_SHA"
  else
    BASE="HEAD~1"
  fi
else
  BASE="HEAD~1"
fi

echo "==> Detecting changes (base: ${BASE})"

if ! CHANGED=$(git diff --name-only "$BASE" HEAD 2>/dev/null); then
  echo "==> Could not determine diff — running all pipelines"
  CHANGED="crates/ xtask/ Cargo.toml"
fi

if [[ -n "$CHANGED" ]]; then
  echo "$CHANGED"
else
  echo "  (no files changed)"
fi

# ── Classify changes ─────────────────────────────────────────────────

RUST=false
DOCS=false
BENCH=false
EXAMPLES=false
EDITORS=false

# CI infrastructure or workspace manifest changes trigger all pipelines.
if echo "$CHANGED" | grep -qE '^(\.github/workflows/|ci/|Cargo\.toml$|Cargo\.lock$|rust-toolchain\.toml$)'; then
  echo "==> CI infrastructure or workspace manifest changed — running all pipelines"
  RUST=true
  DOCS=true
  BENCH=true
  EXAMPLES=true
  EDITORS=true
fi

# Editor extensions/integrations (VS Code TS + Zed wasm get built in CI).
if echo "$CHANGED" | grep -qE '^editors/'; then
  EDITORS=true
fi

if echo "$CHANGED" | grep -qE '^(crates/|xtask/|schemas/|\.alint\.yml$)'; then
  RUST=true
fi

# Bench-only changes keep the bench smoke alive but still need rust to build.
if echo "$CHANGED" | grep -qE '^(crates/alint-bench/|xtask/)'; then
  BENCH=true
fi

# `facts.json` (repo root) is a generated contract guarded by `gen-facts
# --check` in the docs job; route it through `docs` so a hand-edit can't skip
# every gate (the architecture artifacts under docs/ already match here).
if echo "$CHANGED" | grep -qE '^(docs/|PROPOSAL\.md$|[A-Z_]+\.md$|facts\.json$)'; then
  DOCS=true
fi

# Examples-validate covers schema regressions in the 30 pinned
# case-study configs. Trigger on examples/ edits, on schema /
# rules / DSL edits that could change accepted shapes, and on
# config-parser edits.
if echo "$CHANGED" | grep -qE '^(examples/|schemas/|crates/alint-rules/|crates/alint-dsl/|crates/alint-core/src/config/)'; then
  EXAMPLES=true
fi

echo ""
echo "==> rust=${RUST}  docs=${DOCS}  bench=${BENCH}  examples=${EXAMPLES}  editors=${EDITORS}"

# ── Write GitHub Actions outputs ─────────────────────────────────────

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "rust=${RUST}"
    echo "docs=${DOCS}"
    echo "bench=${BENCH}"
    echo "examples=${EXAMPLES}"
    echo "editors=${EDITORS}"
  } >> "$GITHUB_OUTPUT"
fi
