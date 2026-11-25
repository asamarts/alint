#!/usr/bin/env bash
# ci/scripts/check-v2-schema-id.sh -- verify the draft v2 config schema `$id`
# contract (#570, ADR-0050 D9):
#
#   Default / `--dev` mode (local preflight and PR CI):
#     1. `xtask/src/gen_schema_v2.rs::RELEASE` matches the tag in
#        `schemas/v2/config.json`'s `$id`.
#     2. Both documented v2 YAML modelines (`docs/site/configuration/version-2.md`
#        and `docs/development/CONFIG-AUTHORING.md`) match that exact `$id` URL.
#
#   `--release <tag>` mode (release.yml preflight on tag pushes):
#     Runs all `--dev` checks, plus:
#     3. `RELEASE` is not ahead of `<tag>`.
#     4. Finds the latest prior `v*.*.*` tag strictly less than `<tag>` that
#        contains `schemas/v2/config.json`.
#     5. Normalizes `$id` out of both `<prev_tag>:schemas/v2/config.json` and
#        working-tree `schemas/v2/config.json`; if the schema body changed (or
#        no prior tag had the v2 schema), requires `RELEASE == <tag>`. If the
#        schema body is identical to `<prev_tag>`, passes without forcing a
#        spurious `$id` bump.
#
# Override the repository root for hermetic tests via `V2_SCHEMA_REPO_ROOT`.

set -euo pipefail

MODE="dev"
RELEASE_TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dev)
      MODE="dev"
      shift
      ;;
    --release)
      if [[ $# -lt 2 ]]; then
        echo "[check-v2-schema-id] --release requires a tag argument (e.g. v0.24.0)" >&2
        exit 2
      fi
      MODE="release"
      RELEASE_TAG="$2"
      shift 2
      ;;
    -h|--help)
      sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "[check-v2-schema-id] unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

SCRIPT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ROOT="${V2_SCHEMA_REPO_ROOT:-$SCRIPT_ROOT}"

python3 - "$ROOT" "$MODE" "$RELEASE_TAG" <<'PY'
import json
import re
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
mode = sys.argv[2]
release_tag = sys.argv[3]

SEMVER_TAG_RE = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
RELEASE_CONST_RE = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+RELEASE:\s*&str\s*=\s*"([^"]+)"\s*;',
    re.MULTILINE,
)
MODELINE_RE = re.compile(
    r"^#\s*yaml-language-server:\s*\$schema=(\S+/schemas/v2/config\.json)\s*$",
    re.MULTILINE,
)
ID_PREFIX = "https://raw.githubusercontent.com/asamarts/alint/"
ID_SUFFIX = "/schemas/v2/config.json"

MODELINE_DOCS = (
    "docs/site/configuration/version-2.md",
    "docs/development/CONFIG-AUTHORING.md",
)


def parse_semver(tag: str):
    m = SEMVER_TAG_RE.match(tag)
    if not m:
        return None
    return tuple(int(part) for part in m.groups())


def fail(msg: str) -> None:
    print(f"[check-v2-schema-id] FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


gen_file = root / "xtask/src/gen_schema_v2.rs"
schema_file = root / "schemas/v2/config.json"
if not gen_file.is_file():
    fail(f"missing {gen_file}")
if not schema_file.is_file():
    fail(f"missing {schema_file}")

gen_text = gen_file.read_text(encoding="utf-8")
m_rel = RELEASE_CONST_RE.search(gen_text)
if not m_rel:
    fail(f"could not find `const RELEASE: &str = \"...\";` in {gen_file}")
schema_release = m_rel.group(1)
schema_release_ver = parse_semver(schema_release)
if schema_release_ver is None:
    fail(f"invalid RELEASE tag {schema_release!r} in {gen_file} (expected vX.Y.Z)")

expected_id = f"{ID_PREFIX}{schema_release}{ID_SUFFIX}"

try:
    cur_schema = json.loads(schema_file.read_text(encoding="utf-8"))
except Exception as exc:
    fail(f"could not parse {schema_file} as JSON: {exc}")

actual_id = cur_schema.get("$id")
if actual_id != expected_id:
    fail(
        f"{schema_file} `$id` is {actual_id!r}, expected {expected_id!r} "
        f"(from xtask/src/gen_schema_v2.rs RELEASE = {schema_release!r}); "
        "run `cargo run -p xtask -- gen-schema`"
    )

for rel_doc in MODELINE_DOCS:
    doc_path = root / rel_doc
    if not doc_path.is_file():
        fail(f"missing documented modeline file {rel_doc}")
    matches = MODELINE_RE.findall(doc_path.read_text(encoding="utf-8"))
    if not matches:
        fail(
            f"{rel_doc} has no `# yaml-language-server: $schema=.../schemas/v2/config.json` "
            "modeline"
        )
    for found in matches:
        if found != expected_id:
            fail(
                f"{rel_doc} modeline names {found!r}, expected {expected_id!r} "
                f"(matching xtask/src/gen_schema_v2.rs RELEASE = {schema_release!r})"
            )

if mode == "dev":
    print(f"[check-v2-schema-id] OK (dev): RELEASE={schema_release}, $id and docs modelines match")
    sys.exit(0)

# --release <tag> mode
release_ver = parse_semver(release_tag)
if release_ver is None:
    fail(f"invalid --release tag {release_tag!r} (expected vX.Y.Z)")

if schema_release_ver > release_ver:
    fail(
        f"xtask/src/gen_schema_v2.rs RELEASE ({schema_release}) is ahead of "
        f"release tag {release_tag}"
    )

tags_proc = subprocess.run(
    ["git", "-C", str(root), "tag", "-l", "v*.*.*"],
    capture_output=True,
    text=True,
)
if tags_proc.returncode != 0:
    fail(f"git tag failed in {root}: {tags_proc.stderr.strip()}")

prior_tags = []
for raw in tags_proc.stdout.splitlines():
    tag = raw.strip()
    ver = parse_semver(tag)
    if ver is not None and ver < release_ver:
        prior_tags.append((ver, tag))
prior_tags.sort(reverse=True)

prev_tag = None
prev_schema = None
for _, candidate in prior_tags:
    show = subprocess.run(
        ["git", "-C", str(root), "show", f"{candidate}:schemas/v2/config.json"],
        capture_output=True,
        text=True,
    )
    if show.returncode == 0:
        prev_tag = candidate
        try:
            prev_schema = json.loads(show.stdout)
        except Exception as exc:
            fail(f"could not parse {candidate}:schemas/v2/config.json as JSON: {exc}")
        break

if prev_tag is None:
    if schema_release != release_tag:
        fail(
            f"no prior v*.*.* tag contains schemas/v2/config.json; "
            f"RELEASE ({schema_release}) must match release tag {release_tag}"
        )
    print(
        f"[check-v2-schema-id] OK (release {release_tag}): first v2 schema release, "
        f"RELEASE={schema_release}"
    )
    sys.exit(0)

cur_body = dict(cur_schema)
prev_body = dict(prev_schema)
cur_body.pop("$id", None)
prev_body.pop("$id", None)

if cur_body != prev_body:
    if schema_release != release_tag:
        fail(
            f"schemas/v2/config.json changed since {prev_tag}, but "
            f"xtask/src/gen_schema_v2.rs RELEASE is still {schema_release!r} "
            f"(expected {release_tag!r}). Bump RELEASE, regenerate via "
            "`cargo run -p xtask -- gen-schema`, and update the v2 schema "
            "modelines in docs/site/configuration/version-2.md and "
            "docs/development/CONFIG-AUTHORING.md (or run ci/scripts/bump-version.sh)."
        )
    print(
        f"[check-v2-schema-id] OK (release {release_tag}): schema changed since "
        f"{prev_tag} and RELEASE/$id/modelines updated to {release_tag}"
    )
else:
    print(
        f"[check-v2-schema-id] OK (release {release_tag}): schema body unchanged since "
        f"{prev_tag} (RELEASE={schema_release})"
    )
PY
