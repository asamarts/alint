#!/usr/bin/env bash
set -euo pipefail

# Validate required environment variables
: "${GITHUB_REPO_URL:?GITHUB_REPO_URL is required}"
: "${GITHUB_TOKEN:?GITHUB_TOKEN is required}"

RUNNER_NAME="${RUNNER_NAME:-alint-runner}"
RUNNER_LABELS="${RUNNER_LABELS:-self-hosted,linux,alint}"
CONFIG_DIR="${RUNNER_CONFIG_DIR:-/home/runner/_config}"

cd /home/runner/actions-runner

CREDENTIAL_FILES=(.runner .credentials .credentials_rsaparams)

# Restore persisted credentials from config volume
if [[ -d "$CONFIG_DIR" && -f "$CONFIG_DIR/.runner" ]]; then
    echo "==> Restoring runner credentials from ${CONFIG_DIR}"
    for f in "${CREDENTIAL_FILES[@]}"; do
        [[ -f "$CONFIG_DIR/$f" ]] && cp "$CONFIG_DIR/$f" .
    done
fi

# Drop credentials whose server-side registration is gone. Without this the agent
# connects, is told "the runner registration has been deleted from the server",
# exits, and `--restart unless-stopped` starts it again forever. Clearing them
# turns that into a normal re-registration below, or the explicit failure there.
discard_stale_credentials() {
    echo "==> The stored registration is no longer valid on the server; discarding it"
    for f in "${CREDENTIAL_FILES[@]}"; do
        rm -f "$f" "${CONFIG_DIR}/${f}"
    done
}

# Register runner if not already configured
if [[ ! -f .runner ]]; then
    MAX_ATTEMPTS=5
    for attempt in $(seq 1 "$MAX_ATTEMPTS"); do
        echo "==> Registering runner '${RUNNER_NAME}' for ${GITHUB_REPO_URL} (attempt ${attempt}/${MAX_ATTEMPTS})"
        if ./config.sh \
            --url "${GITHUB_REPO_URL}" \
            --token "${GITHUB_TOKEN}" \
            --name "${RUNNER_NAME}" \
            --labels "${RUNNER_LABELS}" \
            --unattended \
            --disableupdate \
            --replace; then
            break
        fi
        if [[ "$attempt" -eq "$MAX_ATTEMPTS" ]]; then
            echo "==> Registration failed after ${MAX_ATTEMPTS} attempts. Is GITHUB_TOKEN valid?"
            exit 1
        fi
        delay=$(( 2 ** attempt ))
        echo "==> Registration failed, retrying in ${delay}s..."
        sleep "$delay"
    done

    # Persist credentials to config volume
    if [[ -d "$CONFIG_DIR" ]]; then
        echo "==> Persisting runner credentials to ${CONFIG_DIR}"
        for f in "${CREDENTIAL_FILES[@]}"; do
            [[ -f "$f" ]] && cp "$f" "$CONFIG_DIR/"
        done
    fi
fi

# Stop the agent on a signal. Deliberately NOT deregistering here.
#
# SIGTERM arrives on an ordinary `podman restart`, on `podman stop`, and on host
# shutdown. Removing the registration there invalidates the very identity the
# config volume exists to preserve: the next start restores credentials the
# server has already deleted, and `--restart unless-stopped` turns that into a
# loop. The failure is delayed and confusing, because `config.sh remove` needs a
# still-valid registration token, so a restart in the first hour after setup
# destroys the runner while a restart the next day appears to work.
#
# Found when another project copied this runner pattern: one restart minutes
# after provisioning produced fifteen container restarts and zero registered
# runners. Deregistration is an explicit teardown action
# (`teardown.sh --purge`), never something a routine restart does by accident.
cleanup() {
    echo "==> Caught signal, stopping the runner agent"
    kill -TERM "${AGENT_PID:-0}" 2>/dev/null || true
}
trap cleanup SIGTERM SIGINT

echo "==> Starting runner agent"
AGENT_TMP="$(mktemp -d)"
AGENT_LOG="${AGENT_TMP}/agent.log"
AGENT_FIFO="${AGENT_TMP}/agent.fifo"
mkfifo "$AGENT_FIFO"
set +e
# Stream via a named FIFO so AGENT_PID stays `./run.sh` (receiving SIGTERM) AND
# we can explicitly `wait "$TEE_PID"` before reading `$AGENT_LOG`, eliminating
# the race where bash process substitution has not finished flushing on exit.
tee "$AGENT_LOG" < "$AGENT_FIFO" &
TEE_PID=$!
./run.sh > "$AGENT_FIFO" 2>&1 &
AGENT_PID=$!

# Wait again after the handler runs. `wait` returns as soon as a trap fires,
# *before* the child has exited, so a single `wait` lets this script finish while
# the agent is still shutting down and the container stops out from under it.
AGENT_STATUS=0
while :; do
    wait "$AGENT_PID"
    AGENT_STATUS=$?
    kill -0 "$AGENT_PID" 2>/dev/null || break
done
wait "$TEE_PID"
set -e

if grep -qF 'registration has been deleted from the server' "$AGENT_LOG"; then
    discard_stale_credentials
fi
rm -rf "$AGENT_TMP"

exit "$AGENT_STATUS"
