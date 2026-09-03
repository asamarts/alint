#!/usr/bin/env python3
"""Repackage alint's attested release binaries as ``py3-none-<platform>`` wheels.

This is the Path B assembler from ``docs/design/distribution-pypi.md``: for each
release target it takes the release tarball (the byte-identical, SLSA-attested
bytes that ship on the GitHub Release), extracts the ``alint`` / ``alint.exe``
binary, and assembles a Python-version-agnostic wheel that drops that binary onto
PATH via the wheel ``.data/scripts/`` category. No compiler, no PyO3, no
interpreter in the hot path: the wheel is only a delivery vehicle for the exact
same binary the tarball carries, so the release's existing attestation
transitively covers the wheel's payload (see ADR-0015 and section 7 of the design
doc).

Deterministic by construction: fixed zip timestamps, a stable entry order, and no
host state leak into the archive, so the same inputs always produce byte-identical
wheels.

Usage:
    build_wheels.py --tag v0.16.0 --tarball-dir <dir> --out dist

``<dir>`` holds the release tarballs named ``alint-<tag>-<target>.tar.gz`` (as
produced by ``ci/scripts/release-binary.sh`` and uploaded to the Release), each
optionally accompanied by a ``.sha256`` sidecar which, when present, is verified
before the tarball is opened (fail-closed).
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import sys
import tarfile
import zipfile
from pathlib import Path

# Rust release target -> the PEP 425 platform tag(s) the wheel carries. The
# musl-static Linux binaries carry a COMPOUND manylinux + musllinux tag: a
# musllinux-only tag makes pip on ordinary glibc hosts skip the wheel even though
# the static binary runs there fine. macOS floors match the pinned
# MACOSX_DEPLOYMENT_TARGET in release.yml (MP-L7); if that pin moves, move these.
# See distribution-pypi.md section 3 for the full mapping and rationale.
TARGET_PLATFORM_TAGS: dict[str, list[str]] = {
    "x86_64-unknown-linux-musl": ["manylinux_2_17_x86_64", "musllinux_1_2_x86_64"],
    "aarch64-unknown-linux-musl": ["manylinux_2_17_aarch64", "musllinux_1_2_aarch64"],
    "x86_64-apple-darwin": ["macosx_10_12_x86_64"],
    "aarch64-apple-darwin": ["macosx_11_0_arm64"],
    "x86_64-pc-windows-msvc": ["win_amd64"],
}

# License / attribution files copied out of the tarball into the wheel's
# .dist-info so a `pip`-installed alint carries the same notices as the tarball
# (parity with the P1 license bundle; section 6 of the design doc). Order fixed
# for deterministic RECORD output.
LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.html")

NAME = "alint"
SUMMARY = "Lint repository structure, filenames, and file content with a fast, config-driven engine."
HOMEPAGE = "https://alint.org"
SOURCE = "https://github.com/asamarts/alint"
# Low floor on purpose: the payload is a native binary, interpreter-independent,
# so requires-python only gates install-time host selection, never runtime. A
# high floor would needlessly exclude older-Python hosts the binary runs on fine
# (precedent: ruff >=3.7). See section 8 of the design doc.
REQUIRES_PYTHON = ">=3.7"
CLASSIFIERS = (
    "Development Status :: 5 - Production/Stable",
    "Environment :: Console",
    "Intended Audience :: Developers",
    "License :: OSI Approved :: MIT License",
    "License :: OSI Approved :: Apache Software License",
    "Operating System :: POSIX :: Linux",
    "Operating System :: MacOS",
    "Operating System :: Microsoft :: Windows",
    "Topic :: Software Development :: Quality Assurance",
    "Topic :: Utilities",
)

# Fixed DOS epoch (1980-01-01) so wheels are reproducible regardless of when or
# where they are built.
ZIP_EPOCH = (1980, 1, 1, 0, 0, 0)
MODE_EXEC = 0o100755  # regular file, rwxr-xr-x
MODE_DATA = 0o100644  # regular file, rw-r--r--


def record_hash(data: bytes) -> str:
    """RECORD-style digest: ``sha256=<urlsafe-base64, no padding>``."""
    digest = hashlib.sha256(data).digest()
    return "sha256=" + base64.urlsafe_b64encode(digest).rstrip(b"=").decode("ascii")


def find_tarball(tarball_dir: Path, target: str) -> Path:
    """Locate the single ``alint-*-<target>.tar.gz`` for a target (tag-agnostic)."""
    matches = sorted(tarball_dir.glob(f"alint-*-{target}.tar.gz"))
    if not matches:
        raise FileNotFoundError(f"no release tarball for {target} in {tarball_dir}")
    if len(matches) > 1:
        raise RuntimeError(f"ambiguous tarballs for {target}: {[str(m) for m in matches]}")
    return matches[0]


def verify_sha256(tarball: Path) -> None:
    """If a ``.sha256`` sidecar exists, verify the tarball against it (fail-closed)."""
    sidecar = tarball.with_name(tarball.name + ".sha256")
    if not sidecar.exists():
        print(f"  [wheel] note: no .sha256 sidecar for {tarball.name}; skipping verify")
        return
    # `sha256sum` format: "<hex>  <filename>"; take the first field.
    expected = sidecar.read_text().split()[0].strip().lower()
    actual = hashlib.sha256(tarball.read_bytes()).hexdigest()
    if actual != expected:
        raise RuntimeError(f"sha256 mismatch for {tarball.name}: expected {expected}, got {actual}")
    print(f"  [wheel] verified {tarball.name} against .sha256")


def extract_payload(tarball: Path, is_windows: bool) -> tuple[bytes, dict[str, bytes]]:
    """Return ``(binary_bytes, {license_name: bytes})`` from a release tarball.

    The tarball lays out ``alint-<tag>-<target>/alint[.exe]`` plus side-by-side
    license files (see release-binary.sh). The binary bytes are copied verbatim
    so the wheel payload stays byte-identical to the attested release binary.
    """
    bin_leaf = "alint.exe" if is_windows else "alint"
    binary: bytes | None = None
    licenses: dict[str, bytes] = {}
    with tarfile.open(tarball, "r:gz") as tf:
        for member in tf.getmembers():
            if not member.isfile():
                continue
            leaf = member.name.rsplit("/", 1)[-1]
            if leaf == bin_leaf and binary is None:
                fh = tf.extractfile(member)
                assert fh is not None
                binary = fh.read()
            elif leaf in LICENSE_FILES and leaf not in licenses:
                fh = tf.extractfile(member)
                assert fh is not None
                licenses[leaf] = fh.read()
    if binary is None:
        raise RuntimeError(f"{tarball.name}: no {bin_leaf} found inside the archive")
    return binary, licenses


def metadata(version: str, long_description: str | None) -> bytes:
    """Hand-write PEP 566 (Metadata-Version 2.1) METADATA."""
    lines = [
        "Metadata-Version: 2.1",
        f"Name: {NAME}",
        f"Version: {version}",
        f"Summary: {SUMMARY}",
        f"Home-page: {HOMEPAGE}",
        "License: MIT OR Apache-2.0",
        f"Project-URL: Homepage, {HOMEPAGE}",
        f"Project-URL: Source, {SOURCE}",
        f"Project-URL: Documentation, {HOMEPAGE}/docs/",
        f"Requires-Python: {REQUIRES_PYTHON}",
    ]
    lines += [f"Classifier: {c}" for c in CLASSIFIERS]
    if long_description is not None:
        lines.append("Description-Content-Type: text/markdown")
    body = "\n".join(lines) + "\n"
    if long_description is not None:
        body += "\n" + long_description
    return body.encode("utf-8")


def wheel_metadata(platform_tags: list[str]) -> bytes:
    """Hand-write the ``.dist-info/WHEEL`` file (one ``Tag:`` per expanded tag)."""
    lines = [
        "Wheel-Version: 1.0",
        "Generator: alint build_wheels.py (1.0)",
        "Root-Is-Purelib: false",
    ]
    lines += [f"Tag: py3-none-{t}" for t in platform_tags]
    return ("\n".join(lines) + "\n").encode("utf-8")


def add_entry(
    zf: zipfile.ZipFile,
    record: list[tuple[str, str, int]],
    arcname: str,
    data: bytes,
    mode: int,
) -> None:
    """Write one deterministic zip entry and append its RECORD row."""
    zi = zipfile.ZipInfo(arcname, date_time=ZIP_EPOCH)
    zi.external_attr = mode << 16
    zi.compress_type = zipfile.ZIP_DEFLATED
    zf.writestr(zi, data)
    record.append((arcname, record_hash(data), len(data)))


def build_wheel(
    version: str,
    target: str,
    binary: bytes,
    licenses: dict[str, bytes],
    long_description: str | None,
    out_dir: Path,
) -> Path:
    """Assemble one wheel for ``target`` and return its path."""
    platform_tags = TARGET_PLATFORM_TAGS[target]
    is_windows = "windows" in target
    compound = ".".join(platform_tags)  # dot-joined compressed tag set for the filename
    dist_info = f"{NAME}-{version}.dist-info"
    data_scripts = f"{NAME}-{version}.data/scripts"
    bin_leaf = "alint.exe" if is_windows else "alint"

    wheel_name = f"{NAME}-{version}-py3-none-{compound}.whl"
    out_dir.mkdir(parents=True, exist_ok=True)
    wheel_path = out_dir / wheel_name

    record: list[tuple[str, str, int]] = []
    with zipfile.ZipFile(wheel_path, "w", zipfile.ZIP_DEFLATED) as zf:
        # 1. the binary (executable bit set for POSIX installers)
        add_entry(zf, record, f"{data_scripts}/{bin_leaf}", binary, MODE_EXEC)
        # 2. metadata
        add_entry(zf, record, f"{dist_info}/METADATA", metadata(version, long_description), MODE_DATA)
        add_entry(zf, record, f"{dist_info}/WHEEL", wheel_metadata(platform_tags), MODE_DATA)
        # 3. bundled license / attribution text, in a fixed order
        for leaf in LICENSE_FILES:
            if leaf in licenses:
                add_entry(zf, record, f"{dist_info}/licenses/{leaf}", licenses[leaf], MODE_DATA)
        # 4. RECORD last: it lists every entry above, then itself with empty digest/size
        record_lines = [f"{name},{h},{size}" for name, h, size in record]
        record_lines.append(f"{dist_info}/RECORD,,")
        record_blob = ("\n".join(record_lines) + "\n").encode("utf-8")
        zi = zipfile.ZipInfo(f"{dist_info}/RECORD", date_time=ZIP_EPOCH)
        zi.external_attr = MODE_DATA << 16
        zi.compress_type = zipfile.ZIP_DEFLATED
        zf.writestr(zi, record_blob)

    return wheel_path


def main() -> int:
    ap = argparse.ArgumentParser(description="Assemble alint py3-none wheels (Path B).")
    ap.add_argument("--tag", required=True, help="release tag, e.g. v0.16.0")
    ap.add_argument("--tarball-dir", required=True, type=Path, help="dir of release tarballs")
    ap.add_argument("--out", default=Path("dist"), type=Path, help="output dir for wheels")
    ap.add_argument(
        "--targets",
        nargs="*",
        default=list(TARGET_PLATFORM_TAGS),
        help="subset of targets to build (default: all)",
    )
    args = ap.parse_args()

    version = args.tag[1:] if args.tag.startswith("v") else args.tag
    print(f"==> build_wheels: alint {version} (tag {args.tag}) from {args.tarball_dir}")

    long_description: str | None = None
    built: list[Path] = []
    for target in args.targets:
        if target not in TARGET_PLATFORM_TAGS:
            print(f"  [wheel] unknown target {target}; skipping", file=sys.stderr)
            continue
        tarball = find_tarball(args.tarball_dir, target)
        verify_sha256(tarball)
        binary, licenses = extract_payload(tarball, "windows" in target)
        # Use the first target's README (identical across targets) as the PyPI
        # long description, if the tarball carried one.
        if long_description is None:
            readme = _read_readme(tarball)
            if readme is not None:
                long_description = readme
        wheel = build_wheel(version, target, binary, licenses, long_description, args.out)
        print(f"  [wheel] {wheel.name}  ({len(binary):,} B binary, {len(licenses)} license file(s))")
        built.append(wheel)

    if not built:
        print("==> build_wheels: no wheels built", file=sys.stderr)
        return 1
    print(f"==> build_wheels: {len(built)} wheel(s) in {args.out}")
    return 0


def _read_readme(tarball: Path) -> str | None:
    with tarfile.open(tarball, "r:gz") as tf:
        for member in tf.getmembers():
            if member.isfile() and member.name.rsplit("/", 1)[-1] == "README.md":
                fh = tf.extractfile(member)
                if fh is not None:
                    return fh.read().decode("utf-8", errors="replace")
    return None


if __name__ == "__main__":
    raise SystemExit(main())
