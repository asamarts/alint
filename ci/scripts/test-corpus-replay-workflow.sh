#!/usr/bin/env bash
# Pin the corpus replay's R4/R5 feature contract: the replay binary carries
# shadow-ir, while xtask (which shares the bench-record feature graph) does not.
set -euo pipefail
cd "$(dirname "$0")/../.." || exit 1

workflow=.github/workflows/corpus-replay.yml
fail=0

if ! grep -qF 'cargo build --release --locked -p alint --features alint-rules/shadow-ir' "$workflow"; then
  echo "corpus-replay.yml: replay alint is not built with alint-rules/shadow-ir" >&2
  fail=1
fi
if ! grep -qF 'cargo build --release --locked -p xtask' "$workflow"; then
  echo "corpus-replay.yml: xtask is not built separately" >&2
  fail=1
fi
if grep -qE 'cargo build --release --locked -p alint -p xtask' "$workflow"; then
  echo "corpus-replay.yml: a combined package build can unify shadow-ir into xtask" >&2
  fail=1
fi
if ! grep -qF 'alint-rules/shadow-ir' "$workflow"; then
  echo "corpus-replay.yml: shadow-ir is not mentioned" >&2
  fail=1
fi

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi
echo "[corpus-replay-workflow] OK - replay binary has shadow-ir; xtask stays feature-free"
