#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CI_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
if [[ -f "${CI_DIR}/.env" ]]; then
    set -a
    # shellcheck source=/dev/null
    source "${CI_DIR}/.env"
    set +a
fi

CONTAINER_NAME="${CONTAINER_NAME:-alint-runner}"
RUNNER_NAME="${RUNNER_NAME:-alint-runner-$(hostname -s)}"

# Remove the reboot-persistence unit first, so systemd doesn't try to recreate
# the container we're about to tear down. The unit has no ExecStop, so disabling
# it never stops a running container on its own.
echo "==> Removing systemd user unit"
systemctl --user disable --now "container-${CONTAINER_NAME}.service" 2>/dev/null || true
rm -f "${HOME}/.config/systemd/user/container-${CONTAINER_NAME}.service"
systemctl --user daemon-reload 2>/dev/null || true

echo "==> Stopping runner container: ${CONTAINER_NAME}"
podman stop -t 30 "${CONTAINER_NAME}" 2>/dev/null || true

echo "==> Removing container"
podman rm "${CONTAINER_NAME}" 2>/dev/null || true

if [[ "${1:-}" == "--purge" ]]; then
    echo "==> Purging cache and config volumes"
    podman volume rm alint-runner-cargo-cache 2>/dev/null || true
    podman volume rm alint-runner-cargo-target 2>/dev/null || true
    podman volume rm alint-runner-config 2>/dev/null || true
    echo "==> Volumes purged"

    # Deregistration lives here, not in the container's signal handler: a SIGTERM
    # arrives on any ordinary restart, and removing the registration there leaves
    # the runner looping on credentials the server has deleted. Only --purge
    # means "this runner is going away". Through the API rather than
    # `config.sh remove`, whose registration token has expired by teardown time.
    if command -v gh >/dev/null 2>&1 && [[ -n "${GITHUB_REPO_URL:-}" ]]; then
        _slug="${GITHUB_REPO_URL#https://github.com/}"
        _id="$(gh api "repos/${_slug}/actions/runners" \
                 --jq ".runners[] | select(.name==\"${RUNNER_NAME}\") | .id" 2>/dev/null || true)"
        if [[ -n "${_id}" ]]; then
            gh api -X DELETE "repos/${_slug}/actions/runners/${_id}" >/dev/null 2>&1 \
                && echo "==> Deregistered runner '${RUNNER_NAME}'" \
                || echo "==> Could not deregister; remove it in Settings -> Actions -> Runners"
        fi
    else
        echo "==> Remove the runner in Settings -> Actions -> Runners if it lingers"
    fi
fi

echo "==> Teardown complete"
