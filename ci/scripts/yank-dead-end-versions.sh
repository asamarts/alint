#!/usr/bin/env bash
# One-shot yank script — marks `alint-core@0.8.0` and
# `alint-core@0.8.1` as yanked on crates.io. Both got
# uploaded by partial-publish runs of the v0.8.0 / v0.8.1
# release pipelines (see CHANGELOG entries for both versions);
# neither has companion alint-dsl / alint-rules / alint-output
# crates at the same version, so anyone pinning alint-core to
# 0.8.0 or 0.8.1 hits a resolution dead-end downstream. Yanking
# signals "use 0.7.0 or jump to 0.8.2+."
#
# Requires a crates.io token with `yank` scope. Generate one at
# https://crates.io/settings/tokens, then:
#
#   CARGO_REGISTRY_TOKEN=<token> ci/scripts/yank-dead-end-versions.sh
#
# Revoke the token after the run completes.
#
# Idempotent: re-running on already-yanked versions surfaces a
# clean "already yanked" message from cargo and does not abort.
# Companion to `yank-internal-crates.sh` from the v0.8 audit;
# this is the smaller, version-specific cleanup.
set -uo pipefail

if [[ -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "==> ERROR: CARGO_REGISTRY_TOKEN is unset" >&2
  echo "    Generate a token with yank scope at https://crates.io/settings/tokens" >&2
  echo "    then re-run: CARGO_REGISTRY_TOKEN=<token> $0" >&2
  exit 1
fi

# crate@version pairs to yank.
TARGETS=(
  alint-core:0.8.0
  alint-core:0.8.1
)

total=${#TARGETS[@]}
done=0
failed=0
already=0

echo "==> Yanking ${total} dead-end version(s)"
for entry in "${TARGETS[@]}"; do
  echo "    - $entry"
done
echo

for entry in "${TARGETS[@]}"; do
  crate="${entry%%:*}"
  ver="${entry##*:}"
  done=$((done + 1))
  printf "[%d/%d] cargo yank --version %s %s ... " "$done" "$total" "$ver" "$crate"
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
  sleep 0.5
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
