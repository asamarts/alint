#!/usr/bin/env bash
# Install the witforge release that ci/scripts/witforge-consumer.sh pins into
# ROOT/bin (W2-7), and print its version.
#
# Usage: ci/scripts/install-witforge.sh ROOT
#
# The pin is read from witforge-consumer.sh --print-version, so the script is
# the only place it is written. Installing into a fresh ROOT (CI passes a
# directory under $RUNNER_TEMP) needs no cache and leaves ~/.cargo/bin alone:
# `cargo install` refuses a binary in its destination that its own install
# records do not list, which a cached ~/.cargo/bin without those records
# would trigger.
set -euo pipefail

if [[ $# -ne 1 || -z "$1" ]]; then
  echo "usage: $0 ROOT" >&2
  exit 2
fi
root=$1
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="$(bash "$REPO_ROOT/ci/scripts/witforge-consumer.sh" --print-version)"
cargo install witforge-cli --version "$version" --locked --root "$root"
installed="$("$root/bin/witforge" --version)"
if [[ "$installed" != "witforge ${version}" ]]; then
  echo "::error title=witforge install::expected witforge ${version}, installed: ${installed}" >&2
  exit 1
fi
echo "$installed"
