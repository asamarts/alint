#!/usr/bin/env bash
# One-time owner bootstrap for the npm platform packages (ADR-0031). The owner
# runs this locally, logged in to npm (`npm login`, 2FA), before the first
# release that ships a new row of npm/platforms.json.
#
# release.yml's publish-npm job publishes tokenlessly through npm Trusted
# Publishing, and npm can only configure a Trusted Publisher for a package that
# already exists. So for each platform package this script:
#   1. publishes a placeholder 0.0.0 if the package does not exist yet, then
#      waits until the registry serves it: npm accepts a new package's first
#      publish (HTTP 202) and processes it for a few minutes, and until then
#      `npm deprecate` and `npm trust` answer 404;
#   2. deprecates the placeholder unless it already is (nothing depends on it:
#      the shim pins each platform package at its own exact version);
#   3. configures its Trusted Publisher (repository asamarts/alint, workflow
#      release.yml) unless one is already there.
# Idempotent: re-running skips what is done and finishes a package an earlier
# run published but did not deprecate or trust. --dry-run prints the changes
# without making them.
#
# Usage: ci/scripts/npm-bootstrap-platform-packages.sh [--dry-run]
# NPM_BOOTSTRAP_POLL_SECONDS (10) and NPM_BOOTSTRAP_POLL_TRIES (90) bound the
# wait in step 1.
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
if ! npm whoami >/dev/null 2>&1; then
  echo "not logged in to npm; run 'npm login' first" >&2
  exit 1
fi

mapfile -t rows < <(node -e '
  for (const p of require("./npm/platforms.json").packages) console.log([p.name, p.os, p.cpu, p.label].join("\t"));
')
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

for row in "${rows[@]}"; do
  IFS=$'\t' read -r name os cpu label <<<"$row"
  echo "==> ${name}"
  placeholder_published=0
  if published "$name"; then
    echo "    exists on npm"
  else
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
    placeholder_published=1
    [[ "$DRY_RUN" == 1 ]] || wait_until_published "$name"
  fi

  if [[ "$DRY_RUN" == 1 && "$placeholder_published" == 1 ]] || undeprecated_placeholder "$name"; then
    run npm deprecate "${name}@0.0.0" "Placeholder that reserved the name; install ${SHIM} instead."
  fi

  if npm trust list "$name" --json 2>/dev/null | grep -q "$GITHUB_REPO" &&
     npm trust list "$name" --json 2>/dev/null | grep -q "$WORKFLOW"; then
    echo "    Trusted Publisher already configured"
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
