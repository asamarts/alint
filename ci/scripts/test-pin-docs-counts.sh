#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PINNER="$REPO_ROOT/ci/scripts/pin-docs-counts.py"
FIXTURE="$(mktemp -d)"
trap 'rm -rf -- "$FIXTURE"' EXIT

cp "$REPO_ROOT/docs/site/concepts/start-here/kinds-families-categories.md" "$FIXTURE/page.md"

# facts v2 counted accepted spellings; the pin must recover canonical kinds by
# subtracting the alias map and must also use the release's category membership.
python3 - "$REPO_ROOT/facts.json" "$FIXTURE/facts-v2.json" <<'PY'
import json
import sys

source, destination = sys.argv[1:]
facts = json.load(open(source, encoding="utf-8"))
facts["format_version"] = 2
facts["counts"].pop("rule_aliases", None)
# Model the v0.17 surface: remove the post-release kind and its secondary tag.
facts["rule_kinds"].remove("markdown_links_resolve")
facts["rule_categories"].pop("markdown_links_resolve")
facts["counts"]["rule_kinds"] = len(facts["rule_kinds"])
json.dump(facts, open(destination, "w", encoding="utf-8"))
PY

python3 "$PINNER" "$FIXTURE/facts-v2.json" "$FIXTURE/page.md"
python3 "$PINNER" "$FIXTURE/facts-v2.json" "$FIXTURE/page.md" --check
grep -Fq '94 rule kinds plus 11 aliases' "$FIXTURE/page.md"
grep -Fq '32 kinds carry extra tags' "$FIXTURE/page.md"
grep -Fq 'and 90 more' "$FIXTURE/page.md"

# The current v3 contract is canonical-counted and should leave the source page
# byte-for-byte unchanged.
cp "$REPO_ROOT/docs/site/concepts/start-here/kinds-families-categories.md" "$FIXTURE/page-v3.md"
python3 "$PINNER" "$REPO_ROOT/facts.json" "$FIXTURE/page-v3.md" --check

# Check mode must reject a page that drifted ahead of its release facts.
sed -i '0,/94 rule kinds plus 11 aliases/s//95 rule kinds plus 11 aliases/' "$FIXTURE/page.md"
if python3 "$PINNER" "$FIXTURE/facts-v2.json" "$FIXTURE/page.md" --check >/dev/null 2>&1; then
  echo "[pin-docs-counts] checker accepted a main-only count" >&2
  exit 1
fi

echo "[pin-docs-counts] OK — v2/v3 contracts pin and drift fails"
