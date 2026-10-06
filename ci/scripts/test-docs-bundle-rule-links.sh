#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHECKER="$REPO_ROOT/ci/scripts/check-docs-bundle-rule-links.py"
FIXTURE="$(mktemp -d)"
trap 'rm -rf -- "$FIXTURE"' EXIT

mkdir -p "$FIXTURE/rules/content" "$FIXTURE/rules/cross-file"

cat > "$FIXTURE/rules/index.md" <<'EOF'
# Rules

- [Content](/docs/rules/content/)
EOF

cat > "$FIXTURE/rules/content/index.md" <<'EOF'
# Content

- [file_exists](/docs/rules/content/file_exists/)
- [External](https://example.com/docs/rules/missing/)
EOF

cat > "$FIXTURE/rules/cross-file/index.md" <<'EOF'
# Cross-file

No entries yet.
EOF

cat > "$FIXTURE/rules/content/file_exists.md" <<'EOF'
# file_exists
EOF

python3 "$CHECKER" "$FIXTURE"

cat >> "$FIXTURE/rules/cross-file/index.md" <<'EOF'

- [unreleased](/docs/rules/cross-file/unreleased/#options)
EOF

if output="$(python3 "$CHECKER" "$FIXTURE" 2>&1)"; then
  echo "[docs-bundle-rule-links] checker accepted a missing release page" >&2
  exit 1
fi
if ! grep -Fq \
  "rules/cross-file/index.md: /docs/rules/cross-file/unreleased/#options" \
  <<<"$output"; then
  echo "[docs-bundle-rule-links] checker reported the wrong failure:" >&2
  echo "$output" >&2
  exit 1
fi

echo "[docs-bundle-rule-links] OK — valid links pass and an unreleased page fails"
