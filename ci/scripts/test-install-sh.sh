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

# Documented one-liners must fail on HTTP errors (-f), not pipe an error page
# into bash. Scan the docs this repo owns.
if grep -rnE 'curl -sSL[^|]*install\.sh' README.md install.sh docs/site 2>/dev/null; then
  echo "[test-install-sh] use 'curl -fsSL' (not -sSL) for the install one-liner" >&2
  fail=1
fi

# The post-publish install.sh smoke must prove verification RAN: it requires
# verification and asserts the exact success line install.sh prints.
if ! grep -q 'ALINT_REQUIRE_VERIFY=1' ci/scripts/smoke-channel.sh ||
   ! grep -q "'^==> Signature OK'" ci/scripts/smoke-channel.sh ||
   ! grep -q 'echo "==> Signature OK' install.sh; then
  echo "[test-install-sh] smoke-channel.sh must run install.sh with ALINT_REQUIRE_VERIFY=1 and assert '==> Signature OK'" >&2
  fail=1
fi

[[ "$fail" -eq 0 ]] && echo "[test-install-sh] OK - every truncated prefix of install.sh executes nothing"
exit "$fail"
