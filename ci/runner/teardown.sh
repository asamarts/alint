#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME="${CONTAINER_NAME:-alint-runner}"

echo "==> Stopping runner container: ${CONTAINER_NAME}"
podman stop -t 30 "${CONTAINER_NAME}" 2>/dev/null || true

echo "==> Removing container"
podman rm "${CONTAINER_NAME}" 2>/dev/null || true

if [[ "${1:-}" == "--purge" ]]; then
    echo "==> Purging cache volumes"
    podman volume rm alint-runner-cargo-cache 2>/dev/null || true
    podman volume rm alint-runner-cargo-target 2>/dev/null || true
    echo "==> Volumes purged"
fi

echo "==> Teardown complete"
