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
  if ! out="$("$@" 2>&1)"; then
    echo "  [smoke] FAIL: ${CHANNEL} command '$*' exited non-zero: ${out}" >&2
    return 1
  fi
  echo "  [smoke] ${CHANNEL}: ${out}"
  # Match the `alint <ver>` line wherever it is -- a channel may emit a warning
  # line first, so keying on field 2 of a merged-stderr line 1 is fragile.
  got="$(printf '%s\n' "$out" | awk '/^alint [0-9]/ { print $2; exit }')"
  if [ "$got" != "$VER" ]; then
    echo "  [smoke] FAIL: ${CHANNEL} served version '${got:-<none>}', expected '${VER}'" >&2
    return 1
  fi
  echo "  [smoke] OK: ${CHANNEL} serves alint ${VER}"
}

# When smoking the LATEST release (SMOKE_FLOATING=1, set by the workflow when the
# resolved tag == the latest release), also confirm a channel's FLOATING pointer
# (its unversioned / :latest install) resolves to VER, so a stale npm dist-tag or
# docker :latest is caught, not just the pinned install. No-op otherwise.
check_floating() {
  [[ "${SMOKE_FLOATING:-}" == "1" ]] || return 0
  local got="$1"
  if [ "$got" != "$VER" ]; then
    echo "  [smoke] FAIL: ${CHANNEL} 'latest' pointer serves '${got:-<none>}', expected '${VER}'" >&2
    return 1
  fi
  echo "  [smoke] OK: ${CHANNEL} latest pointer -> ${VER}"
}

echo "==> smoke: channel=${CHANNEL} tag=${TAG} ver=${VER} floating=${SMOKE_FLOATING:-0}"
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
    # --disable-strategies compile: never silently fall back to a SOURCE build, so
    # a missing / renamed / corrupt pre-built asset fails loudly (the point of
    # MP-M5; binstall otherwise compiles from crates.io and exits 0, hiding it).
    retry cargo binstall --no-confirm --disable-strategies compile "alint@${VER}"
    assert_version alint --version
    ;;
  npm)
    retry npm install -g "@asamarts/alint@${VER}"
    assert_version alint --version
    check_floating "$(npm view "@asamarts/alint" version 2>/dev/null)"
    ;;
  docker)
    retry "$DOCKER" pull "${IMAGE}:${VER}"
    assert_version "$DOCKER" run --rm "${IMAGE}:${VER}" --version
    check_floating "$("$DOCKER" run --rm "${IMAGE}:latest" --version 2>&1 | awk '/^alint [0-9]/ { print $2; exit }')"
    ;;
  homebrew)
    # Homebrew serves only the LATEST formula (no version pin), so smoke it only
    # when the resolved tag IS the latest (SMOKE_FLOATING=1); an older-tag manual
    # dispatch would install the newer latest and false-fail. Retry the whole
    # install+check so a tap lagging the just-published tag is ridden out (a bare
    # `brew install` succeeds against the stale formula, which retrying only the
    # install would miss).
    if [[ "${SMOKE_FLOATING:-}" != "1" ]]; then
      echo "  [smoke] homebrew: skipped (smoked tag ${TAG} is not the latest release)"
      exit 0
    fi
    # Homebrew refuses formulae from third-party taps it deems "untrusted" in
    # non-interactive CI. v0.15.1's smoke hit exactly this -- "Refusing to load
    # formula asamarts/alint/alint from untrusted tap asamarts/alint" -- and the
    # `brew install` never landed alint, so the later `alint --version` was
    # command-not-found (exit 127). The leg already taps; the gate is trust, not
    # order. Cover the two documented gates: read the formula from the local tap
    # clone (NO_INSTALL_FROM_API), and add our tap to the allowlist IF the runner
    # set one (leaving it unset when it was unset, so we don't turn on an
    # allowlist that would then forbid alint's own deps). The diagnostic line
    # prints brew's version + the allowlist so a still-red run pinpoints the gate.
    export HOMEBREW_NO_INSTALL_FROM_API=1
    [ -n "${HOMEBREW_ALLOWED_TAPS:-}" ] && export HOMEBREW_ALLOWED_TAPS="${HOMEBREW_ALLOWED_TAPS} asamarts/alint"
    brew tap asamarts/alint 2>/dev/null || true
    echo "  [smoke] brew=$(brew --version 2>/dev/null | head -1) HOMEBREW_ALLOWED_TAPS='${HOMEBREW_ALLOWED_TAPS:-<unset>}'"
    hb_n=1
    while true; do
      brew update >/dev/null 2>&1 || true
      brew reinstall alint >/dev/null 2>&1 || brew install alint || true
      hb_got="$(alint --version 2>&1 | awk '/^alint [0-9]/ { print $2; exit }')"
      [ "$hb_got" = "$VER" ] && break
      if [ "$hb_n" -ge "$RETRY_MAX" ]; then
        echo "  [smoke] FAIL: homebrew serves '${hb_got:-<none>}', expected '${VER}' after ${RETRY_MAX} attempts" >&2
        exit 1
      fi
      echo "  [smoke] homebrew attempt ${hb_n}/${RETRY_MAX}: got '${hb_got:-<none>}', want '${VER}'; ${RETRY_SLEEP}s" >&2
      sleep "$RETRY_SLEEP"
      hb_n=$((hb_n + 1))
    done
    assert_version alint --version
    ;;
  *)
    echo "  [smoke] unknown channel: ${CHANNEL}" >&2
    exit 2
    ;;
esac
