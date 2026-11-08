#!/usr/bin/env python3
"""Gate for ci/scripts/stage_npm_packages.py (the npm package stager).

Fabricates release tarballs in the exact release-binary.sh layout, with binaries
whose headers name each platform, runs the stager, and asserts the staged
packages: byte-identical executable binaries, os/cpu/preferUnplugged manifests,
license files, and a shim stamped with exact-version optionalDependencies and no
scripts. Then the fail-closed paths: a missing tarball or sidecar, a checksum
mismatch, a binary for the wrong platform, and drift between the shim's
optionalDependencies and npm/platforms.json. When npm is on PATH, `npm pack
--dry-run` confirms what each package would publish. Pure stdlib otherwise.

Invoked by ci/scripts/test-stage-npm-packages.sh via ci/scripts/shell-tests.sh.
"""
import hashlib
import io
import json
import os
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "ci/scripts/stage_npm_packages.py"
TAG = "v9.9.9"  # a version that cannot collide with a real fixture
VER = "9.9.9"
LICENSES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.html")
PACKAGES = json.loads((REPO / "npm/platforms.json").read_text())["packages"]

sys.path.insert(0, str(REPO / "ci/scripts"))
from stage_npm_packages import binary_platform  # noqa: E402


def fake_binary(os_name: str, cpu: str, tail: bytes = b"") -> bytes:
    """A minimal executable header for ``(os, cpu)``, padded to a plausible size."""
    if os_name == "linux":
        head = b"\x7fELF" + b"\x02\x01\x01" + b"\x00" * 11 + struct.pack("<H", {"x64": 0x3E, "arm64": 0xB7}[cpu])
    elif os_name == "darwin":
        head = b"\xcf\xfa\xed\xfe" + struct.pack("<I", {"x64": 0x01000007, "arm64": 0x0100000C}[cpu])
    else:
        head = b"MZ" + b"\x00" * 0x3A + struct.pack("<I", 0x40)
        head += b"PE\0\0" + struct.pack("<H", {"x64": 0x8664, "arm64": 0xAA64}[cpu])
    return head + b"\x00" * (128 - len(head)) + tail


def make_tarball(tarball_dir: Path, pkg: dict, binary: bytes | None = None, sidecar: bool = True) -> Path:
    target = pkg["target"]
    top = f"alint-{TAG}-{target}"
    tarball = tarball_dir / f"{top}.tar.gz"
    if binary is None:
        binary = fake_binary(pkg["os"], pkg["cpu"], target.encode())
    with tarfile.open(tarball, "w:gz") as tf:

        def add(name: str, data: bytes, mode: int) -> None:
            ti = tarfile.TarInfo(f"{top}/{name}")
            ti.size = len(data)
            ti.mode = mode
            tf.addfile(ti, io.BytesIO(data))

        add(pkg["binary"], binary, 0o755)
        for name in LICENSES:
            add(name, f"{name} text\n".encode(), 0o644)
        add("README.md", b"# alint\n", 0o644)
        add("docs/ARCHITECTURE.md", b"arch\n", 0o644)  # non-payload member: must be ignored
    if sidecar:
        digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
        (tarball_dir / f"{tarball.name}.sha256").write_text(f"{digest}  {tarball.name}\n")
    return tarball


def make_all(tarball_dir: Path) -> None:
    tarball_dir.mkdir()
    for pkg in PACKAGES:
        make_tarball(tarball_dir, pkg)


def run(tarball_dir: Path, out: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--tag", TAG, "--tarball-dir", str(tarball_dir), "--out", str(out)],
        capture_output=True,
        text=True,
    )


def expect_fail(tdp: Path, name: str, needle: str, mutate) -> None:
    case = tdp / name
    case.mkdir()
    tarballs = case / "tarballs"
    make_all(tarballs)
    mutate(tarballs)
    r = run(tarballs, case / "out")
    assert r.returncode != 0, f"{name}: the stager succeeded\n{r.stdout}"
    assert needle in r.stderr, f"{name}: expected {needle!r} in stderr:\n{r.stderr}"
    print(f"  OK refuses: {name}")


def check_staged(out: Path) -> None:
    shim_src = json.loads((REPO / "npm/package.json").read_text())
    for pkg in PACKAGES:
        d = out / pkg["name"].split("/", 1)[1]
        manifest = json.loads((d / "package.json").read_text())
        assert manifest["name"] == pkg["name"] and manifest["version"] == VER, manifest
        assert manifest["os"] == [pkg["os"]] and manifest["cpu"] == [pkg["cpu"]], manifest
        assert manifest["preferUnplugged"] is True, manifest
        assert "libc" not in manifest, "static musl binaries run on any libc; no libc field"
        for field in ("scripts", "bin", "dependencies", "optionalDependencies"):
            assert field not in manifest, f"{pkg['name']}: unexpected {field}"
        for field in ("license", "repository", "homepage", "bugs"):
            assert manifest[field] == shim_src[field], f"{pkg['name']}: {field} differs from the shim"
        binary = d / "bin" / pkg["binary"]
        assert binary.read_bytes() == fake_binary(pkg["os"], pkg["cpu"], pkg["target"].encode()), (
            f"{pkg['name']}: binary is not byte-identical to the tarball's"
        )
        if os.name == "posix":
            assert binary.stat().st_mode & 0o777 == 0o755, f"{pkg['name']}: mode {oct(binary.stat().st_mode)}"
        for name in LICENSES:
            assert (d / name).read_text() == f"{name} text\n", f"{pkg['name']}: missing {name}"
        assert set(manifest["files"]) == {f"bin/{pkg['binary']}", "README.md", *LICENSES}, manifest["files"]
        assert pkg["target"] in (d / "README.md").read_text()
    shim = json.loads((out / "alint/package.json").read_text())
    assert shim["version"] == VER
    assert shim["optionalDependencies"] == {p["name"]: VER for p in PACKAGES}, shim["optionalDependencies"]
    assert "scripts" not in shim and "dependencies" not in shim, shim
    for rel in shim["files"]:
        assert (out / "alint" / rel).read_bytes() == (REPO / "npm" / rel).read_bytes(), rel
    print(f"  OK staged {len(PACKAGES)} platform packages + the shim")


def check_npm_pack(out: Path) -> None:
    npm = shutil.which("npm")
    if npm is None:
        print("  skip npm pack --dry-run (npm not on PATH)")
        return
    cache = out.parent / "npm-cache"  # keep the user's npm cache clean
    for d in sorted(p for p in out.iterdir() if p.is_dir()):
        r = subprocess.run(
            [npm, "pack", "--dry-run", "--json", "--ignore-scripts", "--cache", str(cache)],
            cwd=d,
            capture_output=True,
            text=True,
            check=True,
        )
        files = {f["path"]: f.get("mode") for f in json.loads(r.stdout)[0]["files"]}
        manifest = json.loads((d / "package.json").read_text())
        assert set(files) == set(manifest["files"]) | {"package.json"}, f"{d.name}: packs {sorted(files)}"
        for path, mode in files.items():
            if path.startswith("bin/alint") and not path.endswith(".js") and not path.endswith(".exe"):
                assert mode is not None and mode & 0o111, f"{d.name}: {path} packs without the exec bit ({mode})"
    print("  OK npm pack --dry-run: each package publishes exactly its files")


def main() -> int:
    for pkg in PACKAGES:
        got = binary_platform(fake_binary(pkg["os"], pkg["cpu"]))
        assert got == (pkg["os"], pkg["cpu"]), f"binary_platform({pkg['name']}) = {got}"
    assert binary_platform(b"#!/bin/sh\necho alint\n") is None
    assert binary_platform(b"\x7fELF" + b"\x00" * 14 + struct.pack("<H", 0x28)) is None  # 32-bit ARM
    print("  OK binary_platform reads ELF, Mach-O and PE headers")

    with tempfile.TemporaryDirectory() as td:
        tdp = Path(td)
        make_all(tdp / "tarballs")
        r = run(tdp / "tarballs", tdp / "out")
        assert r.returncode == 0, f"stager failed:\n{r.stdout}\n{r.stderr}"
        check_staged(tdp / "out")
        check_npm_pack(tdp / "out")

        r = run(tdp / "tarballs", tdp / "out")
        assert r.returncode != 0, "the stager overwrote an existing output dir"
        print("  OK refuses: an existing output dir")

        linux_arm = next(p for p in PACKAGES if p["name"].endswith("linux-arm64"))
        win_arm = next(p for p in PACKAGES if p["name"].endswith("win32-arm64"))
        expect_fail(
            tdp,
            "missing tarball",
            "no release tarball for aarch64-pc-windows-msvc",
            lambda t: [f.unlink() for f in t.glob("*aarch64-pc-windows-msvc*")],
        )
        expect_fail(
            tdp,
            "missing sidecar",
            "no .sha256 sidecar",
            lambda t: next(t.glob("*x86_64-apple-darwin.tar.gz.sha256")).unlink(),
        )
        expect_fail(
            tdp,
            "checksum mismatch",
            "sha256 mismatch",
            lambda t: next(t.glob("*x86_64-apple-darwin.tar.gz.sha256")).write_text("0" * 64 + "  x\n"),
        )
        expect_fail(
            tdp,
            "x64 binary in the linux-arm64 package",
            "not @asamarts/alint-linux-arm64",
            lambda t: make_tarball(t, linux_arm, fake_binary("linux", "x64")),
        )
        expect_fail(
            tdp,
            "x64 binary in the win32-arm64 package",
            "not @asamarts/alint-win32-arm64",
            lambda t: make_tarball(t, win_arm, fake_binary("win32", "x64")),
        )
        expect_fail(
            tdp,
            "a script instead of a binary",
            "built for None",
            lambda t: make_tarball(t, linux_arm, b"#!/bin/sh\necho alint\n"),
        )
    print("[test-stage-npm-packages] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
