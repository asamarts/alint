#!/usr/bin/env python3
"""Stage alint's npm packages from the attested release tarballs.

The npm distribution is one package per platform (``@asamarts/alint-<os>-<cpu>``,
the rows of ``npm/platforms.json``) carrying that platform's native binary, plus
``@asamarts/alint``, a shim that names every platform package as an exact-version
optionalDependency and runs the installed one (ADR-0015, ADR-0031). Like the PyPI
wheels (``build_wheels.py``), each platform package repackages the byte-identical
binary from the GitHub Release tarball, so npm provenance now covers the binary
bytes and they match the release's own build provenance.

Fail-closed, because a published version can never be replaced:
- every platform's tarball must be present with its ``.sha256`` sidecar, and
  match it;
- each binary's header must match its package's ``os`` and ``cpu`` (an x64
  binary in the arm64 package is caught here, not by a user);
- the shim's optionalDependencies must name exactly the table's packages.

Usage:
    stage_npm_packages.py --tag v0.22.1 --tarball-dir <dir> --out <dir>

``--out`` receives one directory per package: ``alint-<os>-<cpu>/`` for each
platform and ``alint/`` for the shim, each ready for ``npm publish``. Publish the
platform packages first and the shim last (``publish-npm.sh`` does).
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

from build_wheels import binary_platform, extract_payload, find_tarball, verify_sha256

REPO = Path(__file__).resolve().parents[2]
NPM_DIR = REPO / "npm"
SHIM_PACKAGE = "@asamarts/alint"
SHIM_LICENSES = ("LICENSE-APACHE", "LICENSE-MIT", "NOTICE")

# Fields every package shares with the shim's package.json.
SHARED_FIELDS = ("homepage", "bugs", "license", "repository")


def package_dir_name(name: str) -> str:
    """``@asamarts/alint-linux-x64`` -> ``alint-linux-x64``."""
    return name.split("/", 1)[1]


def platform_readme(pkg: dict) -> str:
    return (
        f"# {pkg['name']}\n\n"
        f"The [alint](https://github.com/asamarts/alint) binary for {pkg['label']}.\n\n"
        f"Install [`{SHIM_PACKAGE}`](https://www.npmjs.com/package/{SHIM_PACKAGE}) instead: it\n"
        "depends on this package and runs its binary. The binary is byte-identical to\n"
        f"the `{pkg['target']}` archive on the GitHub Release of the same version.\n"
    )


def stage_platform(pkg: dict, tag: str, version: str, shim: dict, tarball_dir: Path, out: Path) -> Path:
    tarball = find_tarball(tarball_dir, tag, pkg["target"])
    verify_sha256(tarball)
    binary, licenses = extract_payload(tarball, tag, pkg["target"], pkg["os"] == "win32")
    if binary_platform(binary) != (pkg["os"], pkg["cpu"]):
        raise RuntimeError(
            f"{tarball.name}: the binary is built for {binary_platform(binary)}, "
            f"not {pkg['name']}'s {(pkg['os'], pkg['cpu'])}"
        )

    dest = out / package_dir_name(pkg["name"])
    (dest / "bin").mkdir(parents=True)
    bin_path = dest / "bin" / pkg["binary"]
    bin_path.write_bytes(binary)
    bin_path.chmod(0o755)
    for leaf, data in sorted(licenses.items()):
        (dest / leaf).write_bytes(data)
    (dest / "README.md").write_text(platform_readme(pkg))

    manifest = {
        "name": pkg["name"],
        "version": version,
        "description": f"The alint binary for {pkg['label']}. Install {SHIM_PACKAGE}, which depends on it.",
        **{field: shim[field] for field in SHARED_FIELDS},
        "os": [pkg["os"]],
        "cpu": [pkg["cpu"]],
        "files": [f"bin/{pkg['binary']}", "README.md", *sorted(licenses)],
        # Yarn PnP runs packages from their zip unless unplugged; a native
        # binary has to exist on disk.
        "preferUnplugged": True,
    }
    (dest / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"  [npm] {pkg['name']}@{version}  ({len(binary):,} B {pkg['target']} binary)")
    return dest


def stage_shim(packages: list[dict], version: str, shim: dict, out: Path) -> Path:
    expected = [p["name"] for p in packages]
    declared = list(shim.get("optionalDependencies", {}))
    if sorted(declared) != sorted(expected):
        raise RuntimeError(
            f"npm/package.json optionalDependencies {declared} != npm/platforms.json packages {expected}"
        )
    if shim.get("scripts"):
        raise RuntimeError("npm/package.json must have no scripts (ADR-0031: no install script)")

    dest = out / package_dir_name(SHIM_PACKAGE)
    dest.mkdir(parents=True)
    for rel in shim["files"]:
        (dest / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(NPM_DIR / rel, dest / rel)
    # Ship Apache-2.0 / MIT license texts and NOTICE inside the shim tarball
    # too (Apache-2.0 §4) so every published npm package carries its notices.
    files = list(shim["files"])
    for leaf in SHIM_LICENSES:
        src = REPO / leaf
        if not src.is_file():
            raise RuntimeError(f"missing root license file: {src}")
        shutil.copy2(src, dest / leaf)
        if leaf not in files:
            files.append(leaf)
    manifest = dict(shim)
    manifest["version"] = version
    manifest["files"] = files
    manifest["optionalDependencies"] = {name: version for name in expected}
    (dest / "package.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"  [npm] {SHIM_PACKAGE}@{version}  (shim; {len(expected)} optional platform packages)")
    return dest


def main() -> int:
    ap = argparse.ArgumentParser(description="Stage alint's npm packages from release tarballs.")
    ap.add_argument("--tag", required=True, help="release tag, e.g. v0.22.1")
    ap.add_argument("--tarball-dir", required=True, type=Path, help="dir of release tarballs")
    ap.add_argument("--out", required=True, type=Path, help="output dir (must not exist)")
    args = ap.parse_args()

    version = args.tag[1:] if args.tag.startswith("v") else args.tag
    packages = json.loads((NPM_DIR / "platforms.json").read_text())["packages"]
    shim = json.loads((NPM_DIR / "package.json").read_text())
    if shim["name"] != SHIM_PACKAGE:
        raise RuntimeError(f"npm/package.json is {shim['name']}, expected {SHIM_PACKAGE}")

    print(f"==> stage_npm_packages: alint {version} (tag {args.tag}) from {args.tarball_dir}")
    args.out.mkdir(parents=True, exist_ok=False)
    for pkg in packages:
        stage_platform(pkg, args.tag, version, shim, args.tarball_dir, args.out)
    stage_shim(packages, version, shim, args.out)
    print(f"==> stage_npm_packages: {len(packages) + 1} package(s) in {args.out}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FileNotFoundError, RuntimeError) as e:
        print(f"stage_npm_packages: error: {e}", file=sys.stderr)
        raise SystemExit(1)
