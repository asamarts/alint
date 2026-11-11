#!/usr/bin/env bash
# One-time owner bootstrap for the npm platform packages (ADR-0031). The owner
# runs this locally, logged in to npm (`npm login`, 2FA), before the first
# release that ships a new row of npm/platforms.json.
#
# release.yml's publish-npm job publishes tokenlessly through npm Trusted
# Publishing, and npm can only configure a Trusted Publisher for a package that
# already exists. So, one step at a time across all the platform packages, this
# script:
#   1. publishes a placeholder 0.0.0 of each package that does not exist yet;
#   2. waits until the registry serves those: npm accepts a new package's first
#      publish (HTTP 202) and processes it for a few minutes, and until then
#      `npm deprecate` and `npm trust` answer 404;
#   3. deprecates each placeholder that is not deprecated yet (nothing depends
#      on it: the shim pins each platform package at its own exact version);
#   4. configures each package's Trusted Publisher (repository asamarts/alint,
#      workflow release.yml) unless one is already there.
# Doing each step for every package before the next keeps the commands that
# need 2FA together, so one approval with npm's "skip 2FA for the next 5
# minutes" option covers most of them.
# Idempotent: re-running skips what is done and finishes whatever an earlier run
# left undone. --dry-run prints the changes without making them.
#
# Usage: ci/scripts/npm-bootstrap-platform-packages.sh [--dry-run]
# NPM_BOOTSTRAP_POLL_SECONDS (10) and NPM_BOOTSTRAP_POLL_TRIES (90) bound the
# wait in step 2.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

GITHUB_REPO=asamarts/alint
WORKFLOW=release.yml
SHIM=@asamarts/alint
DRY_RUN=0
case "${1:-}" in
  "") ;;
  --dry-run) DRY_RUN=1 ;;
  *) echo "usage: $0 [--dry-run]" >&2; exit 2 ;;
esac

POLL_SECONDS=${NPM_BOOTSTRAP_POLL_SECONDS:-10}
POLL_TRIES=${NPM_BOOTSTRAP_POLL_TRIES:-90}

run() { # print, then run unless --dry-run
  echo "+ $*"
  [[ "$DRY_RUN" == 1 ]] || "$@"
}

# --prefer-online: a fresh answer, never the local cache's.
published() { [[ -n "$(npm view "$1" name --prefer-online 2>/dev/null || true)" ]]; }

undeprecated_placeholder() {
  [[ "$(npm view "$1@0.0.0" version --prefer-online 2>/dev/null || true)" == 0.0.0 &&
     -z "$(npm view "$1@0.0.0" deprecated --prefer-online 2>/dev/null || true)" ]]
}

# npm asks for 2FA only when stdin and stdout are a terminal, and otherwise
# fails with EOTP, so a piped `npm trust list` could never see an existing
# Trusted Publisher. Run it on a pseudo-terminal: npm's 2FA prompt still reaches
# the owner (through stderr), and the output is captured. Not --json: npm holds
# the 2FA URL back for the JSON output, so the owner would never see it.
trust_list() {
  python3 -c 'import pty, sys; sys.exit(pty.spawn(sys.argv[1:]) >> 8)' \
    npm trust list "$1" --color=false --no-progress | tee -a /dev/stderr
}

trusted() {
  local out
  out=$(trust_list "$1" | tr -d '\r' || true)
  grep -qx "repository: $GITHUB_REPO" <<<"$out" && grep -qx "file: $WORKFLOW" <<<"$out"
}

wait_until_published() {
  local tries=0
  until published "$1"; do
    if ((++tries > POLL_TRIES)); then
      echo "    the registry still does not serve $1; re-run this script later to finish it" >&2
      exit 1
    fi
    ((tries > 1)) || echo "    waiting for the registry to finish processing it"
    sleep "$POLL_SECONDS"
  done
}

if ! npm trust --help >/dev/null 2>&1; then
  echo "npm $(npm --version) has no 'npm trust'; install a newer npm 11 (npm install -g npm@11)" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required (it gives 'npm trust list' a terminal for its 2FA prompt)" >&2
  exit 1
fi
if ! npm whoami >/dev/null 2>&1; then
  echo "not logged in to npm; run 'npm login' first" >&2
  exit 1
fi

mapfile -t rows < <(node -e '
  for (const p of require("./npm/platforms.json").packages) console.log([p.name, p.os, p.cpu, p.label].join("\t"));
')
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

names=()
new=()
echo "==> 1. placeholders"
for row in "${rows[@]}"; do
  IFS=$'\t' read -r name os cpu label <<<"$row"
  names+=("$name")
  if published "$name"; then
    echo "    ${name}: exists on npm"
    continue
  fi
  echo "    ${name}: publishing the 0.0.0 placeholder"
  dir="$work/${name#@asamarts/}"
  mkdir -p "$dir"
  # shellcheck disable=SC2016  # ${...} below are JavaScript template literals
  NAME="$name" OS="$os" CPU="$cpu" LABEL="$label" SHIM="$SHIM" node -e '
    const fs = require("fs");
    const shim = require("./npm/package.json");
    const { NAME, OS, CPU, LABEL, SHIM } = process.env;
    fs.writeFileSync(process.argv[1] + "/package.json", JSON.stringify({
      name: NAME,
      version: "0.0.0",
      description: `Placeholder that reserved the name of the alint binary package for ${LABEL}. Install ${SHIM}.`,
      license: shim.license,
      repository: shim.repository,
      homepage: shim.homepage,
      os: [OS],
      cpu: [CPU],
      files: ["README.md"],
    }, null, 2) + "\n");
    fs.writeFileSync(process.argv[1] + "/README.md",
      `# ${NAME}\n\nA placeholder. Real versions carry the alint binary for ${LABEL};\n` +
      `install [\`${SHIM}\`](https://www.npmjs.com/package/${SHIM}), which depends on them.\n`);
  ' "$dir"
  if ! (cd "$dir" && run npm publish --access public); then
    echo "    publish failed; if an earlier run published ${name} minutes ago, the registry" >&2
    echo "    may still be processing it: wait, then re-run this script" >&2
    exit 1
  fi
  new+=("$name")
done

if [[ "$DRY_RUN" != 1 && ${#new[@]} -gt 0 ]]; then
  echo "==> 2. waiting for the registry to serve the new packages"
  for name in "${new[@]}"; do
    wait_until_published "$name"
    echo "    ${name}: served"
  done
fi

echo "==> 3. deprecations"
for name in "${names[@]}"; do
  if [[ "$DRY_RUN" == 1 && " ${new[*]} " == *" $name "* ]] || undeprecated_placeholder "$name"; then
    run npm deprecate "${name}@0.0.0" "Placeholder that reserved the name; install ${SHIM} instead."
  fi
done

echo "==> 4. Trusted Publishers"
for name in "${names[@]}"; do
  if trusted "$name"; then
    echo "    ${name}: Trusted Publisher already configured"
  else
    run npm trust github "$name" --file "$WORKFLOW" --repo "$GITHUB_REPO" --allow-publish --yes
  fi
done

echo
if [[ "$DRY_RUN" == 1 ]]; then
  echo "dry run: nothing was changed"
else
  echo "done: each platform package exists and trusts ${GITHUB_REPO}'s ${WORKFLOW}."
  echo "check with: npm trust list <package>"
fi
