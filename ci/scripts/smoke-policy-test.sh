#!/usr/bin/env bash
set -euo pipefail

# Exercise `alint test` through an installed distribution command against a
# fresh downstream package. The optional Docker form mounts the fixture at the
# image's working directory and lets the image entrypoint provide `alint`.
#
# Usage:
#   ci/scripts/smoke-policy-test.sh alint
#   ci/scripts/smoke-policy-test.sh uvx alint@0.20.0
#   ci/scripts/smoke-policy-test.sh --docker docker ghcr.io/asamarts/alint:0.20.0

if [[ "$#" -eq 0 ]]; then
  echo "usage: $0 <alint-command...> | --docker <runtime> <image>" >&2
  exit 2
fi

root="$(mktemp -d)"
stdout="$(mktemp)"
stderr="$(mktemp)"
trap 'rm -rf "$root" "$stdout" "$stderr"' EXIT
mkdir -p "$root/alint-tests"
cat > "$root/alint-tests/distribution-smoke.yml" <<'YAML'
test_version: 1
name: installed distribution runs policy tests
given:
  tree:
    README.md: "# downstream\n"
  config: |
    version: 1
    rules:
      - id: downstream-readme
        kind: file_exists
        paths: README.md
        root_only: true
        level: error
when: [check]
expect:
  - exit: success
    violations: []
YAML

if [[ "$1" == "--docker" ]]; then
  if [[ "$#" -ne 3 ]]; then
    echo "usage: $0 --docker <runtime> <image>" >&2
    exit 2
  fi
  chmod -R a+rX "$root"
  command=("$2" run --rm --volume "$root:/work:ro" --workdir /work "$3")
else
  command=("$@")
fi

if ! (cd "$root" && "${command[@]}" test --format json >"$stdout" 2>"$stderr"); then
  echo "  [smoke] FAIL: installed distribution could not run a policy test" >&2
  cat "$stderr" >&2
  cat "$stdout" >&2
  exit 1
fi

# `python3` on Windows runners can resolve to the Microsoft Store alias stub;
# probe candidates for a working Python 3.8+ interpreter.
PY=""
for candidate in python3 python; do
  if "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
    PY="$candidate"
    break
  fi
done
if [[ -z "$PY" ]]; then
  echo "  [smoke] FAIL: Python 3.8+ is required to validate policy-test JSON output" >&2
  exit 1
fi

"$PY" - "$stdout" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, encoding="utf-8") as handle:
        report = json.load(handle)
except (OSError, json.JSONDecodeError) as error:
    raise SystemExit(f"invalid policy-test JSON report: {error}") from error

expected_summary = {
    "selected": 1,
    "passed": 1,
    "failed": 0,
    "invalid": 0,
    "skipped": 0,
}
if report.get("schema_version") != 1 or report.get("kind") != "policy-test-report":
    raise SystemExit(f"unexpected policy-test report envelope: {report!r}")
if report.get("summary") != expected_summary:
    raise SystemExit(f"unexpected policy-test summary: {report.get('summary')!r}")
tests = report.get("tests")
if not isinstance(tests, list) or len(tests) != 1 or tests[0].get("status") != "passed":
    raise SystemExit(f"unexpected policy-test records: {tests!r}")
PY

echo "  [smoke] OK: installed distribution executes a downstream policy-test package"
