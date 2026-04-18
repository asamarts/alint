#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Load environment
ENV_FILE="${CI_DIR}/.env"
if [[ ! -f "$ENV_FILE" ]]; then
    echo "Error: ${ENV_FILE} not found."
    echo "Copy ci/env.example to ci/.env and fill in the values."
    exit 1
fi
set -a; source "$ENV_FILE"; set +a

: "${GITHUB_REPO_URL:?GITHUB_REPO_URL is required in .env}"
: "${GITHUB_TOKEN:?GITHUB_TOKEN is required in .env}"

RUNNER_IMAGE="${RUNNER_IMAGE:-alint-runner}"
CONTAINER_NAME="${CONTAINER_NAME:-alint-runner}"

echo "==> Building runner image: ${RUNNER_IMAGE}"
podman build -t "${RUNNER_IMAGE}" -f "${CI_DIR}/Containerfile" "${CI_DIR}"

echo "==> Creating volumes"
podman volume create alint-runner-config 2>/dev/null || true
podman volume create alint-runner-cargo-cache 2>/dev/null || true
podman volume create alint-runner-cargo-target 2>/dev/null || true

echo "==> Starting runner container: ${CONTAINER_NAME}"
podman run -d \
    --name "${CONTAINER_NAME}" \
    --restart unless-stopped \
    -e GITHUB_REPO_URL="${GITHUB_REPO_URL}" \
    -e GITHUB_TOKEN="${GITHUB_TOKEN}" \
    -e RUNNER_NAME="${RUNNER_NAME:-alint-runner}" \
    -e RUNNER_LABELS="${RUNNER_LABELS:-self-hosted,linux,alint}" \
    -v alint-runner-config:/home/runner/_config \
    -v alint-runner-cargo-cache:/usr/local/cargo/registry \
    -v alint-runner-cargo-target:/home/runner/_work/_target \
    "${RUNNER_IMAGE}"

echo "==> Runner started. Check status with: podman logs -f ${CONTAINER_NAME}"
