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

# The container PID ceiling — one half of a MATCHED PAIR with the coverage
# build-job cap (ci/scripts/coverage.sh). A parallel `cargo llvm-cov
# --workspace` build exhausts podman's 2048 default and the runner hangs; but
# raising pids ALONE just converts the hang into an OOM-kill, so coverage.sh
# also caps CARGO_BUILD_JOBS. BOTH must stay. The post-run check below fails
# loudly if a future teardown/re-register drops the flag (a documented
# regression class).
PIDS_LIMIT="16384"

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
    `# PID ceiling — 8x podman's 2048 default, still a fork-bomb guard. See` \
    `# the PIDS_LIMIT definition above (matched pair with coverage.sh).` \
    --pids-limit "${PIDS_LIMIT}" \
    -e GITHUB_REPO_URL="${GITHUB_REPO_URL}" \
    -e GITHUB_TOKEN="${GITHUB_TOKEN}" \
    -e RUNNER_NAME="${RUNNER_NAME:-alint-runner}" \
    -e RUNNER_LABELS="${RUNNER_LABELS:-self-hosted,linux,alint}" \
    -v alint-runner-config:/home/runner/_config \
    -v alint-runner-cargo-cache:/usr/local/cargo/registry \
    -v alint-runner-cargo-target:/home/runner/_work/_target \
    "${RUNNER_IMAGE}"

# Fail loudly if the PID ceiling didn't take — a re-register that drops
# --pids-limit silently reintroduces the coverage hang (see PIDS_LIMIT above).
_actual_pids="$(podman inspect --format '{{.HostConfig.PidsLimit}}' "${CONTAINER_NAME}" 2>/dev/null || echo '?')"
if [[ "${_actual_pids}" != "${PIDS_LIMIT}" ]]; then
    echo "ERROR: ${CONTAINER_NAME} PidsLimit is '${_actual_pids}', expected ${PIDS_LIMIT}." >&2
    echo "       The coverage build will hang; recreate with --pids-limit ${PIDS_LIMIT}." >&2
    exit 1
fi
echo "==> Verified PidsLimit=${PIDS_LIMIT}"

echo "==> Runner started. Check status with: podman logs -f ${CONTAINER_NAME}"
