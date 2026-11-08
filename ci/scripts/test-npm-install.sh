#!/usr/bin/env bash
# End-to-end install test for the npm distribution (ADR-0031): stage the seven
# packages with the release stager, publish them to a throwaway local registry
# (Verdaccio, no uplinks, so nothing reaches or comes from npmjs.com), and
# install them the ways users do:
#
#   npm                      the platform package arrives; `alint` runs it
#   npm --ignore-scripts     same (there is no install script to skip)
#   npm --omit=optional      `alint` exits 1 and says how to reinstall
#   pnpm, bun, yarn (PnP)    each resolves the platform package on its own
#
# The host's platform package carries a copy of the running `node` executable as
# its "alint" binary: a real native executable for this OS and CPU, so the
# stager's header check and the shim's spawn path are exercised without a Rust
# build. The other five platform packages carry header-only placeholders.
#
# Not a shell-tests harness (it downloads the package managers): ci.yml's
# Packaging job runs it on Linux and cross-platform.yml on macOS, Windows and
# Windows on ARM. NPM_E2E_MANAGERS narrows the managers (default: all four).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

VERDACCIO=verdaccio@6.11.1
PNPM=pnpm@12.15.0
BUN=bun@1.4.4
YARN=@yarnpkg/cli-dist@4.18.1
MANAGERS="${NPM_E2E_MANAGERS:-npm pnpm bun yarn}"
VER=9.9.9 # cannot collide with a real release
# `python3` on a Windows runner can be the Microsoft Store stub; take the first
# interpreter that really is Python 3.
PY=
for candidate in python3 python; do
  if "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then
    PY=$candidate
    break
  fi
done
[[ -n "$PY" ]] || { echo "[test-npm-install] needs Python 3.8+" >&2; exit 1; }

work="$(mktemp -d)"
# Git Bash: hand native tools (node, python, the package managers) C:/ paths.
if command -v cygpath >/dev/null 2>&1; then work="$(cygpath -m "$work")"; fi
registry_pid=
cleanup() {
  [[ -n "$registry_pid" ]] && kill "$registry_pid" 2>/dev/null || true
  if [[ -n "${NPM_E2E_KEEP:-}" ]]; then echo "kept $work"; else rm -rf "$work"; fi
}
trap cleanup EXIT

pass=0
fail=0
ok() { echo "  ok: $1"; pass=$((pass + 1)); }
bad() { echo "  FAIL: $1" >&2; fail=$((fail + 1)); }

# ── stage the packages from fabricated release tarballs ───────────────────
mkdir -p "$work/tarballs"
host=$(node -p 'process.platform + "-" + process.arch')
"$PY" - "$work/tarballs" "$host" "$(node -p process.execPath)" <<'PY'
import hashlib, io, json, struct, sys, tarfile
from pathlib import Path

out, host, node = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
packages = json.loads(Path("npm/platforms.json").read_text())["packages"]

def placeholder(os_name, cpu):
    if os_name == "linux":
        head = b"\x7fELF\x02\x01\x01" + b"\0" * 11 + struct.pack("<H", {"x64": 0x3E, "arm64": 0xB7}[cpu])
    elif os_name == "darwin":
        head = b"\xcf\xfa\xed\xfe" + struct.pack("<I", {"x64": 0x01000007, "arm64": 0x0100000C}[cpu])
    else:
        head = b"MZ" + b"\0" * 0x3A + struct.pack("<I", 0x40) + b"PE\0\0" + struct.pack("<H", {"x64": 0x8664, "arm64": 0xAA64}[cpu])
    return head + b"\0" * 64

found = False
for p in packages:
    is_host = f"{p['os']}-{p['cpu']}" == host
    found |= is_host
    data = node.read_bytes() if is_host else placeholder(p["os"], p["cpu"])
    top = f"alint-v9.9.9-{p['target']}"
    tarball = out / f"{top}.tar.gz"
    with tarfile.open(tarball, "w:gz") as tf:
        for name, body, mode in ((p["binary"], data, 0o755), ("LICENSE-MIT", b"MIT\n", 0o644)):
            ti = tarfile.TarInfo(f"{top}/{name}")
            ti.size, ti.mode = len(body), mode
            tf.addfile(ti, io.BytesIO(body))
    digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
    (out / f"{tarball.name}.sha256").write_text(f"{digest}  {tarball.name}\n")
if not found:
    sys.exit(f"no npm platform package for this host ({host})")
PY
"$PY" ci/scripts/stage_npm_packages.py --tag "v$VER" --tarball-dir "$work/tarballs" --out "$work/dist" >/dev/null
echo "==> staged $(find "$work/dist" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ') packages for host ${host}"

# ── tools and the local registry ─────────────────────────────────────────
tools="$work/tools"
tool_pkgs=("$VERDACCIO")
for m in $MANAGERS; do
  case "$m" in
    npm) ;;
    pnpm) tool_pkgs+=("$PNPM") ;;
    bun) tool_pkgs+=("$BUN") ;;
    yarn) tool_pkgs+=("$YARN") ;;
    *) echo "unknown manager '$m'" >&2; exit 2 ;;
  esac
done
echo "==> installing ${tool_pkgs[*]}"
npm install --prefix "$tools" --no-fund --no-audit --loglevel=error "${tool_pkgs[@]}" >/dev/null
export PATH="$tools/node_modules/.bin:$PATH"

port=$((20000 + RANDOM % 20000))
reg="http://127.0.0.1:${port}"
mkdir -p "$work/registry"
cat >"$work/registry/config.yaml" <<YAML
storage: ./storage
max_body_size: 500mb
auth:
  htpasswd:
    file: ./htpasswd
uplinks: {}
packages:
  '**':
    access: \$all
    publish: \$authenticated
log: { type: stdout, format: pretty, level: warn }
YAML
node "$tools/node_modules/verdaccio/bin/verdaccio" --config "$work/registry/config.yaml" --listen "127.0.0.1:${port}" \
  >"$work/registry/log" 2>&1 &
registry_pid=$!
for _ in $(seq 1 60); do
  curl -sf "$reg/-/ping" >/dev/null 2>&1 && break
  sleep 1
done
curl -sf "$reg/-/ping" >/dev/null || { cat "$work/registry/log" >&2; echo "registry did not start" >&2; exit 1; }
token=$(curl -sf -X PUT -H 'content-type: application/json' \
  -d '{"name":"e2e","password":"e2e-password"}' "$reg/-/user/org.couchdb.user:e2e" |
  node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).token))')
npmrc="$work/npmrc"
printf 'registry=%s/\n//127.0.0.1:%s/:_authToken=%s\n' "$reg" "$port" "$token" >"$npmrc"
export NPM_CONFIG_USERCONFIG="$npmrc"
# Every package manager's cache and store lives under $work: the packages are
# always version 9.9.9, so a shared cache would serve one run's packages (and
# registry port) to the next, and the caller's caches stay untouched.
export npm_config_cache="$work/cache/npm"
export XDG_CACHE_HOME="$work/cache/xdg" XDG_STATE_HOME="$work/cache/xdg" XDG_DATA_HOME="$work/cache/xdg"
export BUN_INSTALL_CACHE_DIR="$work/cache/bun"
export YARN_GLOBAL_FOLDER="$work/cache/yarn"

# Platform packages first, the shim last, as publish-npm does.
for dir in "$work"/dist/alint-*/ "$work/dist/alint/"; do
  (cd "$dir" && npm publish --registry "$reg/" --loglevel=error >/dev/null)
done
echo "==> published to ${reg}"

# ── installs ─────────────────────────────────────────────────────────────
# With `node` as the binary, `alint --version` prints node's version. (Argument
# forwarding is test-npm-shim.js's job: on Windows these commands pass through
# cmd.exe shims that would mangle quoting tests.)
WANT="$(node --version)"

project() {
  local dir="$work/projects/$1"
  mkdir -p "$dir"
  printf '{ "name": "e2e-%s", "version": "1.0.0", "private": true }\n' "$1" >"$dir/package.json"
  echo "$dir"
}

expect_runs() { # <label> <dir> <command...>
  local label=$1 dir=$2 out
  shift 2
  if out=$(cd "$dir" && "$@" --version 2>&1) && [[ "$(printf '%s' "$out" | tr -d '\r' | tail -n1)" == "$WANT" ]]; then
    ok "$label"
  else
    bad "$label: got: $out"
  fi
}

expect_platform_pkg() { # <label> <dir>
  local got
  got=$(cd "$2" && node -p "require('@asamarts/alint-${host}/package.json').version" 2>/dev/null || true)
  if [[ "$got" == "$VER" ]]; then ok "$1"; else bad "$1 (got '${got}')"; fi
}

for m in $MANAGERS; do
  case "$m" in
    npm)
      d=$(project npm)
      (cd "$d" && npm install --no-fund --no-audit --loglevel=error "@asamarts/alint@$VER" >/dev/null)
      expect_platform_pkg "npm: installs @asamarts/alint-${host} beside the shim" "$d"
      expect_runs "npm: npx alint runs the platform binary" "$d" npx --no-install alint
      others=$(cd "$d" && find node_modules/@asamarts -mindepth 1 -maxdepth 1 -name 'alint-*' | wc -l | tr -d ' ')
      if [[ "$others" == 1 ]]; then
        ok "npm: no other platform's package is installed"
      else
        bad "npm: ${others} platform packages installed"
      fi

      d=$(project npm-ignore-scripts)
      (cd "$d" && npm install --ignore-scripts --no-fund --no-audit --loglevel=error "@asamarts/alint@$VER" >/dev/null)
      expect_runs "npm --ignore-scripts: alint still runs (nothing to skip)" "$d" npx --no-install alint

      d=$(project npm-omit-optional)
      (cd "$d" && npm install --omit=optional --no-fund --no-audit --loglevel=error "@asamarts/alint@$VER" >/dev/null)
      rc=0
      out=$(cd "$d" && npx --no-install alint --version 2>&1) || rc=$?
      if [[ "$rc" == 1 && "$out" == *"--omit=optional"* && "$out" == *"@asamarts/alint-${host}"* ]]; then
        ok "npm --omit=optional: alint exits 1 with the reinstall hint"
      else
        bad "npm --omit=optional: exit ${rc}: ${out}"
      fi
      ;;
    pnpm)
      d=$(project pnpm)
      # pnpm 12 takes its store from here, not from --store-dir.
      printf 'storeDir: "%s"\ncacheDir: "%s"\n' "$work/cache/pnpm-store" "$work/cache/pnpm" >"$d/pnpm-workspace.yaml"
      (cd "$d" && pnpm add --registry "$reg/" "@asamarts/alint@$VER" >/dev/null)
      expect_runs "pnpm: pnpm exec alint runs the platform binary" "$d" pnpm exec alint
      ;;
    bun)
      d=$(project bun)
      (cd "$d" && BUN_CONFIG_REGISTRY="$reg/" bun add "@asamarts/alint@$VER" >/dev/null 2>&1)
      expect_platform_pkg "bun: installs @asamarts/alint-${host}" "$d"
      expect_runs "bun: bun run alint runs the platform binary" "$d" bun run alint
      ;;
    yarn)
      d=$(project yarn)
      # npmMinimalAgeGate: Yarn quarantines versions younger than a day by
      # default, and these were published seconds ago.
      printf 'nodeLinker: pnp\nnpmRegistryServer: "%s"\nunsafeHttpWhitelist: ["127.0.0.1"]\nnpmMinimalAgeGate: 0\nenableTelemetry: false\nenableGlobalCache: false\n' \
        "$reg" >"$d/.yarnrc.yml"
      : >"$d/yarn.lock"
      if ! (cd "$d" && yarn add "@asamarts/alint@$VER" >"$work/yarn.log" 2>&1); then
        cat "$work/yarn.log" >&2
        bad "yarn (PnP): yarn add failed"
        continue
      fi
      expect_runs "yarn (PnP): yarn alint runs the unplugged platform binary" "$d" yarn alint
      ;;
  esac
done

echo "[test-npm-install] $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
