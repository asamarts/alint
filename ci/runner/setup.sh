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
set -a
# shellcheck disable=SC1090  # .env is deployment-local, resolved at runtime
source "$ENV_FILE"
set +a

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
# Pin the resolver this container's DNS is forwarded to (2026-09-11).
#
# `pasta` reads the host's /etc/resolv.conf when a container starts and keeps it
# for the container's life. dhcpcd and tailscaled both rewrite that file, and
# when Tailscale DNS was switched off, 11 of 14 runner containers on this host
# lost DNS at once while keeping full connectivity. Stored per container, so it
# survives restarts; override with RUNNER_DNS_HOST on a host whose LAN differs.
DNS_HOST="${RUNNER_DNS_HOST:-192.168.8.1}"

podman run -d \
    --name "${CONTAINER_NAME}" \
    --network "pasta:--dns-host,${DNS_HOST}" \
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

# ── Reboot persistence ───────────────────────────────────────────────────────
# The container's `--restart unless-stopped` policy plus podman-restart.service
# start it on a NORMAL reboot — but only if the container OBJECT survives. A hard
# reboot that clears the container leaves the runner permanently offline
# (observed: CI went dark for hours until a manual re-setup). A user systemd unit
# closes that gap: it guarantees a boot start AND recreates the container (by
# re-running this script) if it is gone. `oneshot`, not a supervising
# `podman start -a` service, because the container already self-restarts via
# podman; a supervising unit would just loop on an already-running container.
# Skipped when invoked as recovery (ALINT_RUNNER_RECOVERY=1) so the unit's own
# ExecStartPre doesn't re-enter systemctl.
if [[ -z "${ALINT_RUNNER_RECOVERY:-}" ]]; then
    echo "==> Installing systemd user unit (reboot persistence)"
    REPO_ROOT="$(cd "${CI_DIR}/.." && pwd)"
    UNIT_DIR="${HOME}/.config/systemd/user"
    mkdir -p "${UNIT_DIR}"
    cat > "${UNIT_DIR}/container-${CONTAINER_NAME}.service" <<UNIT
[Unit]
Description=${CONTAINER_NAME} self-hosted GitHub Actions runner (podman)
Documentation=${GITHUB_REPO_URL}/tree/main/ci/runner
After=network-online.target podman-restart.service
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStartPre=/bin/sh -c 'podman container exists ${CONTAINER_NAME} || (cd ${REPO_ROOT} && ALINT_RUNNER_RECOVERY=1 bash ci/runner/setup.sh)'
ExecStart=/usr/bin/podman start ${CONTAINER_NAME}

[Install]
WantedBy=default.target
UNIT
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable "container-${CONTAINER_NAME}.service" 2>/dev/null || true
    # Linger so the unit starts at boot without an active login session.
    loginctl enable-linger "$(id -un)" >/dev/null 2>&1 || true
    echo "==> Enabled container-${CONTAINER_NAME}.service (runner now survives reboot)"
fi

echo "==> Runner started. Check status with: podman logs -f ${CONTAINER_NAME}"
