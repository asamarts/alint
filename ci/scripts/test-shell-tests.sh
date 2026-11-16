#!/usr/bin/env bash
# Self-test for shell-tests.sh: a pass counts as passed, an exit 77 from a
# harness in SKIPPABLE as skipped, and an exit 77 from any other harness, or
# any other non-zero status, as a failure.
#
# It also keeps the workspace's test suites out of the harnesses: shell-tests.sh
# runs every ci/scripts/test-*.sh, so a harness that runs `cargo test`,
# `cargo nextest` or `cargo llvm-cov`, itself or through a script it runs,
# reruns a suite that has a CI job of its own (the compare-mode suite, named as
# a harness, took the Shell Tests job from 3 to 12 minutes). Such a script
# needs a name without the `test-` prefix (ci/scripts/yaml-compare.sh). A
# harness may still build a fixture crate (`cargo build` in
# test-build-script-freshness.sh), or run a script under a stubbed cargo that
# runs another subcommand.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

# harness <dir> <name> <exit status>
harness() {
  mkdir -p "$1"
  printf '#!/usr/bin/env bash\nexit %s\n' "$3" > "$1/$2"
  chmod +x "$1/$2"
}

# run <dir> <skippable>: sets $rc and $out.
run() {
  set +e
  out=$(SHELL_TESTS_DIR="$1" SHELL_TESTS_SKIPPABLE="$2" bash "$REPO_ROOT/ci/scripts/shell-tests.sh" 2>&1)
  rc=$?
  set -e
}

# expect <name> <dir> <skippable> <rc> <text expected in the output>
expect() {
  run "$2" "$3"
  if [[ $rc -ne $4 ]]; then
    fail "$1: rc=$rc, expected $4:"
    printf '%s\n' "$out" >&2
  elif ! grep -qF -- "$5" <<<"$out"; then
    fail "$1: output lacks \"$5\":"
    printf '%s\n' "$out" >&2
  else
    echo "ok: $1"
  fi
}

ok="$TMP_ROOT/ok"
harness "$ok" test-pass.sh 0
harness "$ok" test-skips.sh 77
expect "a listed harness that exits 77 is skipped" "$ok" "test-skips.sh" 0 \
  "1 harness(es) passed, 1 skipped"

unlisted="$TMP_ROOT/unlisted"
harness "$unlisted" test-pass.sh 0
harness "$unlisted" test-skips.sh 77
expect "an unlisted harness that exits 77 fails" "$unlisted" "test-other.sh" 1 \
  "test-skips.sh exited 77 but is not in SKIPPABLE"

failing="$TMP_ROOT/failing"
harness "$failing" test-pass.sh 0
harness "$failing" test-fails.sh 3
expect "a harness that exits 3 fails" "$failing" "" 1 "1 of 2 harnesses failed"

# suite_runners <dir>: each test-*.sh under <dir> that runs a workspace test
# suite, with how, one per line. A command is read after joining continued
# lines and dropping comments and the bodies of here-documents (input such as
# a Python program, or a file's text), but for one fed to `bash` or `sh`: at
# the start of a line or after `;`, `&`, `|`, `(` or `$(`, past environment
# assignments and `env`, `exec`, `command`, `time` and `timeout <n>`. A script
# a harness runs (`bash x.sh`, `sh x.sh`, `source x.sh`, `. x.sh`, or a path to
# one, or a variable set to one, as a command) counts when it is under <dir>
# and runs a suite, at any depth.
suite_runners() {
  python3 - "$1" <<'PY'
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
lead = (r'(?:^|[;&|(]|\$\()\s*(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*'
        r'(?:(?:env(?:\s+[A-Za-z_][A-Za-z0-9_]*=\S*)*|exec|command|time|timeout\s+\S+)\s+)*')
suite = re.compile(lead + r'cargo\s+(?:\+\S+\s+)?(test|nextest|llvm-cov)\b', re.MULTILINE)
script = r'["\']?(?:\S*/)?([A-Za-z0-9_.-]+\.sh)["\']?'
called = [
    re.compile(r'(?:^|[\s;&|(])(?:bash|sh|source|\.)\s+' + script, re.MULTILINE),
    re.compile(lead + script + r'(?=\s|$)', re.MULTILINE),
]
assigned = re.compile(r'^\s*(?:local\s+|readonly\s+)?([A-Za-z_][A-Za-z0-9_]*)=' + script + r'\s*$',
                      re.MULTILINE)
variable = re.compile(lead + r'["\']?\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?["\']?(?=\s|$)', re.MULTILINE)


heredoc = re.compile(r'(?<!<)<<(-?)\s*(["\']?)\\?([A-Za-z_][A-Za-z0-9_]*)\2')
to_shell = re.compile(r'(?:^|[;&|(])\s*(?:bash|sh)\b[^<]*<<')


def code(path):
    """The lines of `path` a shell runs: no comments, and no here-document's
    body unless `bash` or `sh` reads it."""
    text = path.read_text(encoding='utf-8').replace('\\\n', ' ')
    lines, bodies = [], []
    for line in text.splitlines():
        if bodies:
            strip, end, shell = bodies[0]
            if (line.lstrip('\t') if strip else line) == end:
                bodies.pop(0)
            elif shell and not line.lstrip().startswith('#'):
                lines.append(line)
            continue
        if line.lstrip().startswith('#'):
            continue
        lines.append(line)
        shell = bool(to_shell.search(line))
        bodies = [(m.group(1) == '-', m.group(3), shell) for m in heredoc.finditer(line)]
    return '\n'.join(lines)


def runs(name, seen):
    """How the script `name` under root runs a suite, or None."""
    path = root / name
    if name in seen or not path.is_file():
        return None
    seen.add(name)
    text = code(path)
    found = suite.search(text)
    if found:
        return f'runs `cargo {found.group(1)}`'
    variables = dict(assigned.findall(text))
    names = [m for pattern in called for m in pattern.findall(text)]
    names += [variables[v] for v in variable.findall(text) if v in variables]
    for other in names:
        how = runs(other, seen)
        if how:
            return f'runs {other}, which {how}'
    return None


for harness in sorted(root.glob('test-*.sh')):
    how = runs(harness.name, set())
    if how:
        print(f'{harness.name} {how}')
PY
}

# expect_runners <name> <dir> <expected output>
expect_runners() {
  local got
  got=$(suite_runners "$2")
  if [[ "$got" == "$3" ]]; then
    echo "ok: $1"
  else
    fail "$1: got:"
    printf '%s\n' "$got" >&2
  fi
}

suites="$TMP_ROOT/suites"
mkdir -p "$suites"
cat >"$suites/test-direct.sh" <<'SH'
#!/usr/bin/env bash
cargo test --workspace --locked
SH
cat >"$suites/test-continued.sh" <<'SH'
#!/usr/bin/env bash
ALINT_YAML_PARSER=compare \
  cargo test --workspace
SH
cat >"$suites/test-nextest.sh" <<'SH'
#!/usr/bin/env bash
set -e; timeout 600 cargo +stable nextest run --workspace
SH
cat >"$suites/test-via.sh" <<'SH'
#!/usr/bin/env bash
bash "$REPO_ROOT/ci/scripts/coverage-run.sh" --x
SH
cat >"$suites/coverage-run.sh" <<'SH'
#!/usr/bin/env bash
cargo llvm-cov --workspace
SH
cat >"$suites/test-variable.sh" <<'SH'
#!/usr/bin/env bash
RUN="$HERE/deeper.sh"
out=$("$RUN" 2>&1)
SH
cat >"$suites/deeper.sh" <<'SH'
#!/usr/bin/env bash
. ./suite.sh
SH
cat >"$suites/suite.sh" <<'SH'
#!/usr/bin/env bash
exec cargo test -p alint-core
SH
# What a harness may do: build a fixture, stub cargo, mention a suite in a
# comment, a string or a here-document that no shell reads, and run a script
# that runs none.
cat >"$suites/test-allowed.sh" <<'SH'
#!/usr/bin/env bash
# cargo test --workspace is the Test job's.
echo "run cargo test yourself" >&2
printf '#!/bin/sh\necho cargo "$*" >>log\n' >"$stub/cargo"
(cd fixture && cargo build -v)
bash "$REPO_ROOT/ci/scripts/quiet.sh"
python3 - <<'PY'
print('cargo test --workspace')
quiet = ('a string',
         'coverage-run.sh runs')
PY
SH
cat >"$suites/quiet.sh" <<'SH'
#!/usr/bin/env bash
cargo mutants --in-place
SH
cat >"$suites/test-heredoc.sh" <<'SH'
#!/usr/bin/env bash
tr a b <<<EOF
bash -s -- --x <<-EOF
	cargo test --workspace
	EOF
SH
# A here-document `bash` reads is code, and a here-string is not a document.
expect_runners "a harness that runs a workspace suite, directly or through a script, is found" \
  "$suites" "test-continued.sh runs \`cargo test\`
test-direct.sh runs \`cargo test\`
test-heredoc.sh runs \`cargo test\`
test-nextest.sh runs \`cargo nextest\`
test-variable.sh runs deeper.sh, which runs suite.sh, which runs \`cargo test\`
test-via.sh runs coverage-run.sh, which runs \`cargo llvm-cov\`"
# The compare-mode suite under its old name would run in Shell Tests again.
renamed="$TMP_ROOT/renamed"
mkdir -p "$renamed"
cp "$REPO_ROOT/ci/scripts/yaml-compare.sh" "$renamed/test-yaml-compare.sh"
expect_runners "the compare-mode suite named as a harness is found" "$renamed" \
  "test-yaml-compare.sh runs \`cargo test\`"
expect_runners "no harness under ci/scripts runs a workspace suite" "$REPO_ROOT/ci/scripts" ""

if [[ $failures -ne 0 ]]; then
  echo "[shell-tests self-test] $failures case(s) failed" >&2
  exit 1
fi
echo "[shell-tests self-test] OK"
