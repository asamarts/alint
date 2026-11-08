#!/usr/bin/env bash
# One-time owner bootstrap for the npm platform packages (ADR-0031). The owner
# runs this locally, logged in to npm (`npm login`, 2FA), before the first
# release that ships a new row of npm/platforms.json.
#
# release.yml's publish-npm job publishes tokenlessly through npm Trusted
# Publishing, and npm can only configure a Trusted Publisher for a package that
# already exists. So for each platform package this script:
#   1. publishes a placeholder 0.0.0 if the package does not exist yet, and
#      deprecates it at once (nothing depends on it: the shim pins each platform
#      package at its own exact version);
#   2. configures its Trusted Publisher (repository asamarts/alint, workflow
#      release.yml) unless one is already there.
# Idempotent: re-running skips what is done. --dry-run prints the changes
# without making them.
#
# Usage: ci/scripts/npm-bootstrap-platform-packages.sh [--dry-run]
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

run() { # print, then run unless --dry-run
  echo "+ $*"
  [[ "$DRY_RUN" == 1 ]] || "$@"
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
  if [[ -n "$(npm view "$name" name 2>/dev/null || true)" ]]; then
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
    (cd "$dir" && run npm publish --access public)
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
