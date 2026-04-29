#!/usr/bin/env bash
# One-shot yank script — marks every historical version of the
# three internal crates (alint-dsl, alint-rules, alint-output) as
# yanked on crates.io. They've always carried the description
# "Internal: Not a stable public API"; this matches the public
# signal to the manifest's `publish = false` change.
#
# Requires a crates.io token with `yank` scope. Generate one at
# https://crates.io/settings/tokens, then:
#
#   CARGO_REGISTRY_TOKEN=<token> ci/scripts/yank-internal-crates.sh
#
# Revoke the token after the run completes.
#
# Idempotent: re-running on already-yanked versions surfaces a
# clean "already yanked" message from cargo and does not abort.
# The `alint-core` and `alint` crates are *not* touched — those
# are the intentionally-public surface.
set -uo pipefail

if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "==> ERROR: CARGO_REGISTRY_TOKEN is unset" >&2
  echo "    Generate a token with yank scope at https://crates.io/settings/tokens" >&2
  echo "    then re-run: CARGO_REGISTRY_TOKEN=<token> $0" >&2
  exit 1
fi

CRATES=(alint-dsl alint-rules alint-output)
VERSIONS=(
  0.1.0
  0.2.0
  0.3.1 0.3.2
  0.4.1 0.4.2 0.4.3 0.4.4 0.4.5 0.4.6 0.4.7 0.4.8 0.4.9 0.4.10
  0.5.8 0.5.9 0.5.10 0.5.11 0.5.12
  0.6.0
  0.7.0
)

total=$(( ${#CRATES[@]} * ${#VERSIONS[@]} ))
done=0
failed=0
already=0

echo "==> Yanking ${total} crate versions (${#CRATES[@]} crates × ${#VERSIONS[@]} versions)"
echo "    Crates: ${CRATES[*]}"
echo

for crate in "${CRATES[@]}"; do
  for ver in "${VERSIONS[@]}"; do
    done=$((done + 1))
    printf "[%2d/%2d] cargo yank --version %s %s ... " "$done" "$total" "$ver" "$crate"
    out=$(cargo yank --version "$ver" "$crate" 2>&1)
    rc=$?
    if [[ $rc -eq 0 ]]; then
      echo "yanked"
    elif echo "$out" | grep -qi "already yanked"; then
      echo "already yanked (skipped)"
      already=$((already + 1))
    else
      echo "FAILED"
      printf '%s\n' "$out" | sed 's/^/         /'
      failed=$((failed + 1))
    fi
    # Be polite to crates.io's rate limiter.
    sleep 0.5
  done
done

echo
echo "==> Summary"
echo "    yanked:          $((done - failed - already))"
echo "    already yanked:  ${already}"
echo "    failed:          ${failed}"

if [[ $failed -ne 0 ]]; then
  echo "==> ${failed} yank(s) failed; see output above" >&2
  exit 1
fi
echo "==> Done. Revoke the token at https://crates.io/settings/tokens"
