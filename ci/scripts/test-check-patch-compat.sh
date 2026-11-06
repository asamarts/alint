#!/usr/bin/env bash
# Controls for check-patch-compat.sh. A copy of the script runs in a fake
# workspace with `curl` and `cargo` stubbed on PATH, so every exit code is
# exercised without the network: argument and caret-line validation, a
# download or resolution failure (2), a compile failure or a sibling from
# outside the workspace (1), and a pass (0). The `cargo check` stub also
# requires what the script must do to each extracted crate: its Cargo.lock
# gone, and a `[workspace]` and `[patch.crates-io]` appended.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

failures=0
fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

stubs="$TMP_ROOT/bin"
mkdir -p "$stubs"
# curl -fsSL ... <url>: a gzipped tar of `<crate>-<version>/` with a manifest
# and a lock, named from the URL.
cat >"$stubs/curl" <<'STUB'
#!/usr/bin/env bash
[[ -n "${STUB_CURL_FAIL:-}" ]] && exit 22
url="${*: -1}"
name="$(basename "$url" .crate)"
src="$(mktemp -d)"
mkdir -p "$src/$name"
printf '[package]\nname = "%s"\n' "${name%-*}" >"$src/$name/Cargo.toml"
echo "# lock" >"$src/$name/Cargo.lock"
tar -czf - -C "$src" "$name"
rm -rf "$src"
STUB
# cargo fetch | check | tree, each failing on request.
cat >"$stubs/cargo" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  fetch) [[ -z "${STUB_FETCH_FAIL:-}" ]] ;;
  check)
    [[ ! -e Cargo.lock ]] || { echo "lock kept" >&2; exit 99; }
    grep -qx '\[workspace\]' Cargo.toml || { echo "no [workspace]" >&2; exit 99; }
    grep -qx '\[patch.crates-io\]' Cargo.toml || { echo "no [patch]" >&2; exit 99; }
    [[ -z "${STUB_CHECK_FAIL:-}" ]]
    ;;
  tree)
    for sibling in alint-core alint-dsl alint-rules alint-output; do
      echo "$sibling v$STUB_VERSION ($STUB_ROOT/crates/$sibling)"
    done
    [[ -n "${STUB_FOREIGN:-}" ]] && echo "alint-core v0.21.0"
    true
    ;;
  *) exit 98 ;;
esac
STUB
chmod +x "$stubs/curl" "$stubs/cargo"

# workspace <version>: a fake workspace holding a copy of the script.
workspace() {
  local ws="$TMP_ROOT/ws-$1"
  mkdir -p "$ws/ci/scripts"
  cp "$REPO_ROOT/ci/scripts/check-patch-compat.sh" "$ws/ci/scripts/"
  printf '[workspace.package]\nversion = "%s"\n\n[workspace.dependencies]\n' "$1" \
    >"$ws/Cargo.toml"
  echo "$ws"
}

# expect <rc> <workspace version> <name> [VAR=value...] -- <args...>
expect() {
  local want=$1 version=$2 name=$3 ws rc out
  shift 3
  local env=()
  while [[ $1 != -- ]]; do
    env+=("$1")
    shift
  done
  shift
  ws=$(workspace "$version")
  set +e
  out=$(cd "$TMP_ROOT" && env PATH="$stubs:$PATH" TMPDIR="$TMP_ROOT" \
    STUB_ROOT="$ws" STUB_VERSION="$version" "${env[@]}" \
    bash "$ws/ci/scripts/check-patch-compat.sh" "$@" 2>&1)
  rc=$?
  set -e
  if [[ $rc -ne $want ]]; then
    fail "$name: exit $rc, want $want:"
    printf '%s\n' "$out" >&2
  fi
}

expect 2 0.21.2 "no arguments" --
expect 2 0.21.2 "not a version" -- 0.21
expect 2 0.21.2 "the current version" -- 0.21.2
expect 2 0.21.2 "a later patch" -- 0.21.3
expect 2 0.21.2 "an earlier 0.y line" -- 0.20.5
expect 2 0.21.2 "one bad among good" -- 0.21.0 0.20.1
expect 2 0.22.0-rc.1 "a prerelease workspace" -- 0.21.0
expect 0 0.21.2 "every earlier patch" -- 0.21.0 0.21.1
expect 0 1.3.2 "an earlier minor after 1.0" -- 1.2.0 1.3.1
expect 2 1.3.2 "an earlier major" -- 0.9.0
expect 2 1.3.2 "a later minor after 1.0" -- 1.4.0
expect 2 0.21.2 "a download failure" STUB_CURL_FAIL=1 -- 0.21.1
expect 2 0.21.2 "a resolution failure" STUB_FETCH_FAIL=1 -- 0.21.1
expect 1 0.21.2 "a compile failure" STUB_CHECK_FAIL=1 -- 0.21.1
expect 1 0.21.2 "a sibling from crates.io" STUB_FOREIGN=1 -- 0.21.1

# Nothing is left in the scratch directory.
if compgen -G "$TMP_ROOT/alint-patch-compat.*" >/dev/null; then
  fail "the script left its scratch directory behind"
fi

if [[ $failures -ne 0 ]]; then
  echo "test-check-patch-compat: $failures failure(s)" >&2
  exit 1
fi
echo "test-check-patch-compat: all controls pass"
