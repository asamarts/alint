#!/usr/bin/env bash
# Validate the LikeC4 architecture model (docs/design/architecture/model/).
#
# `likec4 validate` proves the model's structural integrity: every dynamic-view
# step (the behavioral/flow diagrams) references a real, declared element and
# relationship, and there is no syntax or layout drift. This is the gate that
# keeps the hand-authored flows honest. The code-derived `*.gen.c4` fragments are
# byte-gated separately by `xtask gen-model --check` (in docs.sh), and the
# crate-element set is gated against `cargo metadata` by a gen-model test.
# See docs/design/architecture-diagrams.md.
#
# Requires Node (>= 20) for `npx`. Until the self-hosted runner image ships Node
# (ci/Containerfile), this script loudly SKIPS rather than failing CI; once Node
# is present it is a hard gate. The likec4 version is pinned for reproducibility.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

LIKEC4_VERSION="1.58.0"
MODEL_DIR="docs/design/architecture/model"

if ! command -v npx >/dev/null 2>&1; then
  echo "[likec4] WARN: Node/npx not found - SKIPPING architecture-model validation."
  echo "[likec4] Add Node >= 20 to ci/Containerfile and rebuild the runner to enable this gate."
  exit 0
fi

echo "==> likec4 validate ($MODEL_DIR)"
npx -y "likec4@${LIKEC4_VERSION}" validate "$MODEL_DIR"
echo "[likec4] architecture model is valid"
