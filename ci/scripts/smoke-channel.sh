#!/usr/bin/env bash
set -euo pipefail

# Post-publish install smoke (MP-M5): install alint from ONE distribution channel
# and assert `alint --version` reports the expected version. Advisory -- run after
# a release (or on demand) so a broken/stale publish is caught by CI, not a user.
# Each channel retries to ride out registry / CDN propagation lag.
#
# Usage: ci/scripts/smoke-channel.sh <channel> <tag>
#   channel: install.sh | cargo | cargo-binstall | npm | docker | homebrew
#   tag:     the release tag, e.g. v0.16.0
#
# Env overrides: DOCKER (default: docker; set to podman locally),
#                RETRY_MAX (default 6), RETRY_SLEEP seconds (default 30).

CHANNEL="${1:?channel required (install.sh|cargo|cargo-binstall|npm|docker|homebrew)}"
TAG="${2:?tag required (e.g. v0.16.0)}"
VER="${TAG#v}"                        # 0.16.0
IMAGE="ghcr.io/asamarts/alint"
DOCKER="${DOCKER:-docker}"
RETRY_MAX="${RETRY_MAX:-6}"
RETRY_SLEEP="${RETRY_SLEEP:-30}"

# Retry a command until it succeeds, to absorb post-publish propagation lag.
retry() {
  local n=1
  until "$@"; do
    if [ "$n" -ge "$RETRY_MAX" ]; then
      echo "  [smoke] '$*' still failing after ${RETRY_MAX} attempts" >&2
      return 1
    fi
    echo "  [smoke] attempt ${n}/${RETRY_MAX} failed; sleeping ${RETRY_SLEEP}s (propagation lag?)" >&2
    sleep "$RETRY_SLEEP"
    n=$((n + 1))
  done
}

# Run the given `alint --version` command and assert field 2 equals $VER exactly.
# `alint --version` prints "alint <ver> (<hash>, built <date>)".
assert_version() {
  local out got
  out="$("$@" 2>&1)"
  echo "  [smoke] ${CHANNEL}: ${out}"
  got="$(printf '%s\n' "$out" | awk 'NR==1 {print $2; exit}')"
  if [ "$got" != "$VER" ]; then
    echo "  [smoke] FAIL: ${CHANNEL} served version '${got}', expected '${VER}'" >&2
    return 1
  fi
  echo "  [smoke] OK: ${CHANNEL} serves alint ${VER}"
}

echo "==> smoke: channel=${CHANNEL} tag=${TAG} ver=${VER}"
case "$CHANNEL" in
  install.sh)
    retry bash -c "curl -fsSL https://alint.org/install.sh | ALINT_VERSION='${TAG}' bash"
    export PATH="${HOME}/.local/bin:${PATH}"       # install.sh's default INSTALL_DIR
    assert_version alint --version
    ;;
  cargo)
    # Mirror the documented `cargo install alint`; --force makes a retry idempotent.
    retry cargo install alint --version "${VER}" --force
    assert_version alint --version
    ;;
  cargo-binstall)
    retry cargo binstall --no-confirm "alint@${VER}"
    assert_version alint --version
    ;;
  npm)
    retry npm install -g "@asamarts/alint@${VER}"
    assert_version alint --version
    ;;
  docker)
    retry "$DOCKER" pull "${IMAGE}:${VER}"
    assert_version "$DOCKER" run --rm "${IMAGE}:${VER}" --version
    ;;
  homebrew)
    # Homebrew serves only the latest formula, so this is meaningful only when
    # smoking the newest release (the post-publish case), not an older tag.
    brew tap asamarts/alint 2>/dev/null || true
    retry brew install alint
    assert_version alint --version
    ;;
  *)
    echo "  [smoke] unknown channel: ${CHANNEL}" >&2
    exit 2
    ;;
esac
