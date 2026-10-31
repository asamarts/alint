#!/usr/bin/env bash
# Check the committed alint plugin contract and its generated consumer crate
# against witforge (ADR-0025 decision 4).
#
# 1. `witforge check` validates the contract against witforge's WIT subset and
#    its wire lock.
# 2. `rust`, `proto`, and `glue` regenerate with `--check` against the copies
#    committed in crates/alint-witforge-consumer, so a stale or hand-edited
#    file fails. (The crate's build.rs generates the prost and proto3-JSON code
#    from the checked schema, so none of that is committed.)
#
# The crate's tests (round-trips through protobuf and proto3-JSON, and the
# comparison with `alint_core::plugin_api`) run with the workspace tests on
# every CI platform; test-witforge-consumer.sh holds this check's controls.
#
# Usage: WITFORGE_BIN=/path/to/witforge ci/scripts/witforge-consumer.sh [--write]
#   --write   regenerate the committed copies instead of checking them
# WITFORGE_BIN defaults to `witforge` on PATH; its version must be
# WITFORGE_VERSION, the release the committed output was generated with.
set -euo pipefail

WITFORGE_VERSION="0.1.0"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
WITFORGE_BIN="${WITFORGE_BIN:-witforge}"
WIT_ROOT="wit/alint-plugin"
CRATE="crates/alint-witforge-consumer"

mode=(--check)
case "${1:-}" in
  "") ;;
  --write) mode=() ;;
  *) echo "usage: $0 [--write]" >&2; exit 2 ;;
esac

if ! version="$("$WITFORGE_BIN" --version 2>/dev/null)"; then
  echo "::error title=witforge consumer::cannot run witforge (WITFORGE_BIN=${WITFORGE_BIN})" >&2
  exit 1
fi
if [[ "$version" != "witforge ${WITFORGE_VERSION}" ]]; then
  echo "::error title=witforge consumer::expected witforge ${WITFORGE_VERSION}, got: ${version}" >&2
  exit 1
fi

"$WITFORGE_BIN" check "${WIT_ROOT}/v0.1"
"$WITFORGE_BIN" rust "$WIT_ROOT" --out "${CRATE}/src/plain" ${mode[@]+"${mode[@]}"}
"$WITFORGE_BIN" proto "$WIT_ROOT" --out "${CRATE}/proto" ${mode[@]+"${mode[@]}"}
"$WITFORGE_BIN" glue "$WIT_ROOT" --out "${CRATE}/src/glue" \
  --plain-root crate::plain --proto-root crate::proto ${mode[@]+"${mode[@]}"}

echo "the plugin contract and its generated consumer match witforge ${WITFORGE_VERSION}"
