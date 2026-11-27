#!/usr/bin/env bash
# install.sh is run as `curl -fsSL .../install.sh | bash`: bash executes as it
# reads, so a download cut off mid-stream must execute NOTHING. install.sh
# guarantees that by being only definitions until a final `main "$@"` line.
# Pin it behaviourally: feed bash every line-prefix of install.sh on stdin with
# stubbed network/platform tools and assert no prefix reaches a download.
# Also pin the documented one-liner form (`curl -fsSL`, fail on HTTP errors).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

sandbox=$(mktemp -d)
trap 'rm -rf "$sandbox"' EXIT
mkdir -p "$sandbox/bin"
for tool in curl uname tar cosign; do
  # shellcheck disable=SC2016  # literal $* / $STUB_LOG belong in the stub
  printf '#!/bin/sh\necho "%s $*" >> "$STUB_LOG"\nexit 1\n' "$tool" > "$sandbox/bin/$tool"
  chmod +x "$sandbox/bin/$tool"
done

fail=0
last=$(grep -v '^[[:space:]]*$' install.sh | tail -n 1)
if [[ "$last" != 'main "$@"' ]]; then
  echo "[test-install-sh] last line must be 'main \"\$@\"' (got: $last)" >&2
  fail=1
fi

total=$(wc -l < install.sh)
for ((n = 1; n < total; n++)); do
  : > "$sandbox/log"
  head -n "$n" install.sh |
    env -i PATH="$sandbox/bin:/usr/bin:/bin" HOME="$sandbox" STUB_LOG="$sandbox/log" \
      INSTALL_DIR="$sandbox/install" bash > "$sandbox/out" 2>&1 || true
  if [[ -s "$sandbox/log" ]] || grep -q '^==>' "$sandbox/out"; then
    echo "[test-install-sh] a download truncated to $n/$total lines EXECUTED code:" >&2
    cat "$sandbox/log" "$sandbox/out" >&2
    fail=1
    break
  fi
done

# The prefix loop above only notices a premature top-level command if it
# reaches a stubbed tool (curl/uname/tar/cosign) or prints a `==>` line; a
# top-level `mkdir`, `cd` or variable assignment would slip through. So also
# pin the SHAPE: run everything except the final `main "$@"` under a DEBUG trap
# (which fires before every simple command, builtins and assignments included,
# but not for function definitions) and require that the only top-level
# command is the leading `set -euo pipefail`.
{
  # shellcheck disable=SC2016  # $BASH_COMMAND / $TOPLOG expand in the child
  printf '%s\n' 'trap '\''printf "%s\n" "$BASH_COMMAND" >> "$TOPLOG"'\'' DEBUG'
  sed '$d' install.sh
} > "$sandbox/toplevel.sh"
: > "$sandbox/toplevel.log"
env -i PATH="$sandbox/bin:/usr/bin:/bin" HOME="$sandbox" STUB_LOG="$sandbox/log" \
  TOPLOG="$sandbox/toplevel.log" bash "$sandbox/toplevel.sh" > "$sandbox/out" 2>&1 || true
toplevel=$(cat "$sandbox/toplevel.log")
if [[ "$toplevel" != 'set -euo pipefail' ]]; then
  echo "[test-install-sh] install.sh must be only 'set -euo pipefail' + function definitions" >&2
  echo "                  before the final 'main \"\$@\"'; top-level commands found:" >&2
  printf '                    %s\n' "${toplevel:-<none: is set -euo pipefail missing?>}" >&2
  fail=1
fi

# With neither INSTALL_DIR nor HOME set, `set -u` used to abort with an opaque
# "HOME: unbound variable"; it must fail with an actionable error BEFORE any
# network access.
: > "$sandbox/log"
if env -i PATH="$sandbox/bin:/usr/bin:/bin" STUB_LOG="$sandbox/log" \
    bash install.sh > "$sandbox/out" 2>&1; then
  echo "[test-install-sh] install.sh succeeded with neither INSTALL_DIR nor HOME set" >&2
  fail=1
elif ! grep -q 'neither INSTALL_DIR nor HOME is set' "$sandbox/out" ||
     grep -q 'unbound variable' "$sandbox/out" || [[ -s "$sandbox/log" ]]; then
  echo "[test-install-sh] unset INSTALL_DIR + HOME must fail early with a clear error:" >&2
  cat "$sandbox/out" "$sandbox/log" >&2
  fail=1
fi

# Documented one-liners must fail on HTTP errors (-f), not pipe an error page
# into bash. Scan the docs this repo owns.
if grep -rnE 'curl -sSL[^|]*install\.sh' README.md install.sh docs/site 2>/dev/null; then
  echo "[test-install-sh] use 'curl -fsSL' (not -sSL) for the install one-liner" >&2
  fail=1
fi

# The post-publish install.sh smoke must prove verification RAN for every
# signed release (v0.15.1+): run its install.sh leg against a stubbed curl that
# serves a fake installer recording ALINT_REQUIRE_VERIFY, and assert the leg is
# strict for signed tags (requires + asserts "==> Signature OK") but does not
# demand a signature from a tag that predates signing (v0.15.0 has no bundle).
if ! grep -q 'echo "==> Signature OK' install.sh; then
  echo "[test-install-sh] install.sh no longer prints '==> Signature OK' (smoke-channel.sh asserts it)" >&2
  fail=1
fi
smoke_leg() { # smoke_leg <tag> <fake installer prints Signature OK: 1|0> -> exit status
  local tag=$1 sig=$2 home="$sandbox/smoke-home"
  rm -rf "$home" "$sandbox/smokebin"
  mkdir -p "$home" "$sandbox/smokebin"
  cat > "$sandbox/smokebin/alint-stub" <<'STUB'
#!/usr/bin/env bash
case "${1:-}" in
  --version)
    echo "alint ${STUB_ALINT_VERSION:?} (stub)"
    ;;
  test)
    echo invoked >> "${STUB_POLICY_LOG:?}"
    cat <<'JSON'
{"schema_version":1,"kind":"policy-test-report","tests":[{"status":"passed"}],"issues":[],"summary":{"selected":1,"passed":1,"failed":0,"invalid":0,"skipped":0}}
JSON
    ;;
  *)
    exit 2
    ;;
esac
STUB
  chmod +x "$sandbox/smokebin/alint-stub"
  cat > "$sandbox/smokebin/curl" <<STUB
#!/usr/bin/env bash
cat <<'INSTALLER'
echo "REQUIRE=\${ALINT_REQUIRE_VERIFY:-unset}" >> "$sandbox/smoke-req"
if [[ "$sig" == 1 ]]; then echo "==> Signature OK (stub)"; fi
mkdir -p "\$HOME/.local/bin"
cp "$sandbox/smokebin/alint-stub" "\$HOME/.local/bin/alint"
INSTALLER
STUB
  chmod +x "$sandbox/smokebin/curl"
  : > "$sandbox/smoke-req"
  : > "$sandbox/smoke-policy-log"
  env -i PATH="$sandbox/smokebin:/usr/bin:/bin" HOME="$home" TMPDIR="${TMPDIR:-/tmp}" \
    STUB_ALINT_VERSION="${tag#v}" STUB_POLICY_LOG="$sandbox/smoke-policy-log" \
    RETRY_MAX=1 RETRY_SLEEP=0 bash ci/scripts/smoke-channel.sh install.sh "$tag" \
    > "$sandbox/smoke-out" 2>&1
}
smoke_case() { # smoke_case <label> <tag> <sig> <want pass|fail> <want REQUIRE> <want policy test 1|0>
  local got=pass policy_ran=0
  smoke_leg "$2" "$3" || got=fail
  [[ -s "$sandbox/smoke-policy-log" ]] && policy_ran=1
  if [[ "$got" != "$4" ]] || ! grep -qx "REQUIRE=$5" "$sandbox/smoke-req" ||
     [[ "$policy_ran" != "$6" ]]; then
    echo "[test-install-sh] smoke install.sh leg, $1: want $4 with REQUIRE=$5 and policy-test=$6, got $got with $(cat "$sandbox/smoke-req") and policy-test=$policy_ran" >&2
    sed 's/^/    | /' "$sandbox/smoke-out" >&2
    fail=1
  fi
}
smoke_case "signed release, verified"             v0.17.0  1 pass 1 0
smoke_case "policy-test release, verified"        v0.20.0  1 pass 1 1
smoke_case "signed release, verification absent"  v0.17.0  0 fail 1 0
smoke_case "first signed release is strict"        v0.15.1  0 fail 1 0
smoke_case "pre-signing tag (no bundle)"           v0.15.0  0 pass 0 0
smoke_case "numeric compare (0.9 < 0.15.1)"        v0.9.0   0 pass 0 0
smoke_case "numeric compare (0.100 > 0.15.1)"      v0.100.0 0 fail 1 0

# `latest` resolution: github.com's /releases/latest redirect first (not subject
# to the REST API's per-IP limit, which 403'd installs on shared CI runners),
# the REST API as fallback, each tag validated before it reaches a URL. Stubbed
# curl: the redirect's final URL comes from $REDIRECT_URL (or exit 22 when
# unset), the API body from $API_JSON (or exit 22), and every download fails so
# a run stops right after resolution.
mkdir -p "$sandbox/latestbin"
cat > "$sandbox/latestbin/curl" <<'STUB'
#!/usr/bin/env bash
echo "curl $*" >> "$STUB_LOG"
url="${*: -1}"
case "$url" in
  */releases/latest)
    if [[ "$url" == https://api.github.com/* ]]; then
      [[ -n "${API_JSON:-}" ]] || exit 22
      printf '%s\n' "$API_JSON"
    else
      [[ -n "${REDIRECT_URL:-}" ]] || exit 22
      printf '%s' "$REDIRECT_URL"
    fi ;;
  *) exit 22 ;;
esac
STUB
cat > "$sandbox/latestbin/uname" <<'STUB'
#!/bin/sh
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac
STUB
chmod +x "$sandbox/latestbin/curl" "$sandbox/latestbin/uname"
latest_case() { # latest_case <label> <want tag | FAIL> <want API called: 1|0> [VAR=value ...]
  local label=$1 want=$2 want_api=$3
  shift 3
  : > "$sandbox/log"
  env -i PATH="$sandbox/latestbin:/usr/bin:/bin" HOME="$sandbox" STUB_LOG="$sandbox/log" \
    INSTALL_DIR="$sandbox/install" "$@" bash install.sh > "$sandbox/out" 2>&1 || true
  local api=0 ok=1
  grep -q 'api.github.com' "$sandbox/log" && api=1
  if [[ "$want" == FAIL ]]; then
    grep -q 'error: could not resolve the latest release tag' "$sandbox/out" || ok=0
    grep -q 'releases/download' "$sandbox/log" && ok=0
  else
    grep -qx "==> Latest version: $want" "$sandbox/out" || ok=0
    grep -q "releases/download/$want/" "$sandbox/log" || ok=0
  fi
  [[ "$api" == "$want_api" ]] || ok=0
  grep -q -- '--retry 3' <(grep 'releases/latest' "$sandbox/log") || ok=0
  if [[ "$ok" != 1 ]]; then
    echo "[test-install-sh] latest resolution, $label: want $want (api=$want_api), got api=$api:" >&2
    sed 's/^/    | /' "$sandbox/out" "$sandbox/log" >&2
    fail=1
  fi
}
gh=https://github.com/asamarts/alint/releases
latest_case "redirect resolves, API untouched" v9.8.7 0 REDIRECT_URL="$gh/tag/v9.8.7"
latest_case "pre-release suffix accepted" v1.2.3-rc.1 0 REDIRECT_URL="$gh/tag/v1.2.3-rc.1"
latest_case "redirect fails, API fallback" v9.8.6 1 API_JSON='{"url":"x","tag_name":"v9.8.6"}'
latest_case "pretty-printed API JSON" v9.8.3 1 \
  API_JSON=$'{\n  "url": "https://api.github.com/x",\n  "tag_name": "v9.8.3",\n  "name": "v9.8.3"\n}'
latest_case "no releases (redirect lands on /releases)" v9.8.5 1 \
  REDIRECT_URL="$gh" API_JSON='{"tag_name":"v9.8.5"}'
latest_case "malformed redirect tag rejected" v9.8.4 1 \
  REDIRECT_URL="$gh/tag/v1.0.0;id" API_JSON='{"tag_name":"v9.8.4"}'
latest_case "both fail" FAIL 1
latest_case "both malformed" FAIL 1 REDIRECT_URL="$gh/tag/..%2F" API_JSON='{"tag_name":"../../etc"}'

explicit_case() { # explicit_case <label> <version> <VALID|INVALID>
  local label=$1 version=$2 want=$3 ok=1
  : > "$sandbox/log"
  env -i PATH="$sandbox/latestbin:/usr/bin:/bin" HOME="$sandbox" STUB_LOG="$sandbox/log" \
    INSTALL_DIR="$sandbox/install" ALINT_VERSION="$version" \
    bash install.sh > "$sandbox/out" 2>&1 || true
  if [[ "$want" == VALID ]]; then
    grep -q "releases/download/$version/" "$sandbox/log" || ok=0
    grep -q 'releases/latest' "$sandbox/log" && ok=0
  else
    grep -q "error: invalid ALINT_VERSION '$version'" "$sandbox/out" || ok=0
    [[ ! -s "$sandbox/log" ]] || ok=0
  fi
  if [[ "$ok" != 1 ]]; then
    echo "[test-install-sh] explicit version, $label: expected $want:" >&2
    sed 's/^/    | /' "$sandbox/out" "$sandbox/log" >&2
    fail=1
  fi
}
explicit_case "stable tag" v9.8.7 VALID
explicit_case "pre-release plus build tag" v1.2.3-rc.1+build.5 VALID
explicit_case "branch name" main INVALID
explicit_case "path-like tag" v1.2.3/../../asset INVALID
explicit_case "incomplete tag" v1.2 INVALID

# Fixed release assets retry transport failures that curl's portable
# `--retry` set does not cover (for example exit 35, a TLS connection reset).
mkdir -p "$sandbox/retrybin" "$sandbox/retry-state"
cat > "$sandbox/retrybin/curl" <<'STUB'
#!/usr/bin/env bash
out=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -o) out=$2; shift 2 ;;
    *) url=$1; shift ;;
  esac
done
case "$url" in
  *.tar.gz)
    count_file="$STUB_STATE/archive-count"
    count=0
    [[ ! -f "$count_file" ]] || read -r count < "$count_file"
    count=$((count + 1))
    printf '%s\n' "$count" > "$count_file"
    [[ "$count" -gt 1 ]] || exit 35
    printf 'archive\n' > "$out"
    ;;
  *.tar.gz.sha256)
    printf '0000000000000000000000000000000000000000000000000000000000000000  %s\n' "$(basename "${url%.sha256}")" > "$out"
    ;;
  *) exit 22 ;;
esac
STUB
cat > "$sandbox/retrybin/uname" <<'STUB'
#!/bin/sh
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac
STUB
cat > "$sandbox/retrybin/sha256sum" <<'STUB'
#!/bin/sh
exit 0
STUB
cat > "$sandbox/retrybin/tar" <<'STUB'
#!/bin/sh
mkdir -p alint-v9.8.7-x86_64-unknown-linux-musl
printf '#!/bin/sh\nexit 0\n' > alint-v9.8.7-x86_64-unknown-linux-musl/alint
STUB
cat > "$sandbox/retrybin/sleep" <<'STUB'
#!/bin/sh
exit 0
STUB
chmod +x "$sandbox/retrybin/"*
if ! env -i PATH="$sandbox/retrybin:/usr/bin:/bin" HOME="$sandbox" \
  STUB_STATE="$sandbox/retry-state" INSTALL_DIR="$sandbox/retry-install" \
  ALINT_VERSION=v9.8.7 bash install.sh > "$sandbox/retry-out" 2>&1 ||
  [[ $(cat "$sandbox/retry-state/archive-count") != 2 ]] ||
  ! grep -q 'Download failed (attempt 1/3)' "$sandbox/retry-out" ||
  [[ ! -x "$sandbox/retry-install/alint" ]]; then
  echo "[test-install-sh] release archive transport failure was not retried:" >&2
  sed 's/^/    | /' "$sandbox/retry-out" >&2
  fail=1
fi

# M5 hardening checks:
# 1. Invalid ALINT_REPO is rejected before any network call.
: > "$sandbox/log"
if env -i PATH="$sandbox/latestbin:/usr/bin:/bin" HOME="$sandbox" STUB_LOG="$sandbox/log" \
  INSTALL_DIR="$sandbox/install" ALINT_REPO="evil/repo/extra" ALINT_VERSION=v9.8.7 \
  bash install.sh > "$sandbox/out" 2>&1 ||
  ! grep -q "error: invalid ALINT_REPO" "$sandbox/out" || [[ -s "$sandbox/log" ]]; then
  echo "[test-install-sh] invalid ALINT_REPO was not rejected early:" >&2
  cat "$sandbox/out" "$sandbox/log" >&2
  fail=1
fi

# 2. Sidecar .sha256 naming a different file is rejected even when sha256sum passes.
mkdir -p "$sandbox/m5-badsha"
cp "$sandbox/retrybin/"* "$sandbox/m5-badsha/"
cat > "$sandbox/m5-badsha/curl" <<'STUB'
#!/usr/bin/env bash
out=""
while [[ $# -gt 0 ]]; do
  case "$1" in -o) out=$2; shift 2 ;; *) url=$1; shift ;; esac
done
case "$url" in
  *.tar.gz) printf 'archive\n' > "$out" ;;
  *.tar.gz.sha256) printf '0000000000000000000000000000000000000000000000000000000000000000  decoy.tar.gz\n' > "$out" ;;
  *) exit 22 ;;
esac
STUB
chmod +x "$sandbox/m5-badsha/curl"
if env -i PATH="$sandbox/m5-badsha:/usr/bin:/bin" HOME="$sandbox" \
  INSTALL_DIR="$sandbox/m5-install" ALINT_VERSION=v9.8.7 \
  bash install.sh > "$sandbox/m5-out" 2>&1 ||
  ! grep -q "names 'decoy.tar.gz'" "$sandbox/m5-out"; then
  echo "[test-install-sh] mismatched .sha256 sidecar filename was not rejected:" >&2
  cat "$sandbox/m5-out" >&2
  fail=1
fi

# 3. Cosign without leading `v` in GitVersion (e.g. `GitVersion: 3.0.2`), exact
#    `--certificate-identity`, atomic replace over a running binary, and
#    regex metacharacters (`+`, `[1]`) in INSTALL_DIR.
mkdir -p "$sandbox/m5-cosign" "$sandbox/m5-dir+regex[1]"
cp "$sandbox/retrybin/uname" "$sandbox/retrybin/tar" "$sandbox/retrybin/sleep" "$sandbox/m5-cosign/"
cat > "$sandbox/m5-cosign/curl" <<'STUB'
#!/usr/bin/env bash
out=""
while [[ $# -gt 0 ]]; do
  case "$1" in -o) out=$2; shift 2 ;; *) url=$1; shift ;; esac
done
case "$url" in
  *.tar.gz) printf 'archive-payload\n' > "$out" ;;
  *.tar.gz.sha256)
    h=$(sha256sum "${out%.sha256}" | awk '{print $1}')
    printf '%s  %s\n' "$h" "$(basename "${url%.sha256}")" > "$out"
    ;;
  */SHA256SUMS)
    h=$(sha256sum "alint-v9.8.7-x86_64-unknown-linux-musl.tar.gz" | awk '{print $1}')
    printf '%s  alint-v9.8.7-x86_64-unknown-linux-musl.tar.gz\n' "$h" > "$out"
    ;;
  */SHA256SUMS.cosign.bundle) printf '{"bundle":true}\n' > "$out" ;;
  *) exit 22 ;;
esac
STUB
cat > "$sandbox/m5-cosign/cosign" <<'STUB'
#!/usr/bin/env bash
if [[ "$1" == "version" ]]; then
  echo "GitVersion:    3.0.2"
  exit 0
fi
printf '%s\n' "$*" > "$COSIGN_ARGS_FILE"
exit 0
STUB
chmod +x "$sandbox/m5-cosign/curl" "$sandbox/m5-cosign/cosign"
# Hold an existing ELF binary executing at INSTALL_DIR/alint so non-atomic cp would fail with ETXTBSY.
cp "$(command -v sleep)" "$sandbox/m5-dir+regex[1]/alint"
"$sandbox/m5-dir+regex[1]/alint" 15 &
busy_pid=$!
if ! env -i PATH="$sandbox/m5-cosign:/usr/bin:/bin" HOME="$sandbox" \
  COSIGN_ARGS_FILE="$sandbox/cosign.args" \
  INSTALL_DIR="$sandbox/m5-dir+regex[1]" ALINT_VERSION=v9.8.7 ALINT_REQUIRE_VERIFY=1 \
  bash install.sh > "$sandbox/m5-cosign-out" 2>&1; then
  kill "$busy_pid" 2>/dev/null || true
  wait "$busy_pid" 2>/dev/null || true
  echo "[test-install-sh] M5 cosign/atomic-install/metachar INSTALL_DIR failed:" >&2
  cat "$sandbox/m5-cosign-out" >&2
  fail=1
else
  kill "$busy_pid" 2>/dev/null || true
  wait "$busy_pid" 2>/dev/null || true
  if ! grep -Fq -- '--certificate-identity https://github.com/asamarts/alint/.github/workflows/release.yml@refs/tags/v9.8.7' "$sandbox/cosign.args" ||
     grep -Fq -- '--certificate-identity-regexp' "$sandbox/cosign.args" ||
     ! grep -q '==> Signature OK' "$sandbox/m5-cosign-out"; then
    echo "[test-install-sh] M5 cosign did not pass exact --certificate-identity:" >&2
    cat "$sandbox/cosign.args" "$sandbox/m5-cosign-out" >&2
    fail=1
  fi
fi

[[ "$fail" -eq 0 ]] && echo "[test-install-sh] OK - every truncated prefix of install.sh executes nothing"
exit "$fail"
