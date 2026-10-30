#!/usr/bin/env bash
# Validate the committed alint plugin contract through a witforge candidate.
#
# This is the pre-release consumer check used before alint moves to the
# published `alint-plugin-api` crate. It stages nothing: the contract already
# lives in a `v<major>.<minor>` directory.
#
# Usage: WITFORGE_BIN=/path/to/witforge ci/scripts/witforge-consumer.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${WITFORGE_BIN:?set WITFORGE_BIN to the witforge candidate binary}"
WIT_ROOT="${ROOT}/wit/alint-plugin"
VERSION_DIR="${WIT_ROOT}/v0.1"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/alint-witforge.XXXXXX")"
trap 'rm -rf "${TMP}"' EXIT

"${WITFORGE_BIN}" check "${VERSION_DIR}"
"${WITFORGE_BIN}" rust "${WIT_ROOT}" --out "${TMP}/plain"
"${WITFORGE_BIN}" proto "${WIT_ROOT}" --out "${TMP}/proto"
"${WITFORGE_BIN}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto
"${WITFORGE_BIN}" rust "${WIT_ROOT}" --out "${TMP}/plain" --check
"${WITFORGE_BIN}" proto "${WIT_ROOT}" --out "${TMP}/proto" --check
"${WITFORGE_BIN}" glue "${WIT_ROOT}" --out "${TMP}/glue" \
    --plain-root crate::plain --proto-root crate::proto --check

cargo test -p alint-witforge-consumer --quiet

echo "alint plugin contract passed the witforge 0.1 consumer check"
