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
  cat > "$sandbox/smokebin/curl" <<STUB
#!/usr/bin/env bash
cat <<'INSTALLER'
echo "REQUIRE=\${ALINT_REQUIRE_VERIFY:-unset}" >> "$sandbox/smoke-req"
if [[ "$sig" == 1 ]]; then echo "==> Signature OK (stub)"; fi
mkdir -p "\$HOME/.local/bin"
printf '#!/usr/bin/env bash\necho "alint %s (stub)"\n' "\${ALINT_VERSION#v}" > "\$HOME/.local/bin/alint"
chmod +x "\$HOME/.local/bin/alint"
INSTALLER
STUB
  chmod +x "$sandbox/smokebin/curl"
  : > "$sandbox/smoke-req"
  env -i PATH="$sandbox/smokebin:/usr/bin:/bin" HOME="$home" TMPDIR="${TMPDIR:-/tmp}" \
    RETRY_MAX=1 RETRY_SLEEP=0 bash ci/scripts/smoke-channel.sh install.sh "$tag" > "$sandbox/smoke-out" 2>&1
}
smoke_case() { # smoke_case <label> <tag> <sig> <want: pass|fail> <want REQUIRE=...>
  local got=pass
  smoke_leg "$2" "$3" || got=fail
  if [[ "$got" != "$4" ]] || ! grep -qx "REQUIRE=$5" "$sandbox/smoke-req"; then
    echo "[test-install-sh] smoke install.sh leg, $1: want $4 with REQUIRE=$5, got $got with $(cat "$sandbox/smoke-req")" >&2
    sed 's/^/    | /' "$sandbox/smoke-out" >&2
    fail=1
  fi
}
smoke_case "signed release, verified"           v0.17.0 1 pass 1
smoke_case "signed release, verification absent" v0.17.0 0 fail 1
smoke_case "first signed release is strict"      v0.15.1 0 fail 1
smoke_case "pre-signing tag (no bundle)"         v0.15.0 0 pass 0
smoke_case "numeric compare (0.9 < 0.15.1)"      v0.9.0  0 pass 0
smoke_case "numeric compare (0.100 > 0.15.1)"    v0.100.0 0 fail 1

[[ "$fail" -eq 0 ]] && echo "[test-install-sh] OK - every truncated prefix of install.sh executes nothing"
exit "$fail"
