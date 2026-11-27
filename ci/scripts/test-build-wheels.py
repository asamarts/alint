#!/usr/bin/env python3
"""Gate for ci/scripts/build_wheels.py (the Path B wheel assembler).

Fabricates release tarballs in the exact release-binary.sh layout, runs the
assembler, and asserts the wheels are spec-correct and reproducible. Pure stdlib,
no network, no venv: fast enough to live in the shell-tests gate. End-to-end
`pip install` + `twine check` are exercised in local dev; the invariants a wheel's
correctness depends on (platform tags, executable bit, byte-identical payload,
RECORD integrity, determinism) are all asserted here.

Invoked by ci/scripts/test-build-wheels.sh via ci/scripts/shell-tests.sh.
"""
import base64
import hashlib
import io
import re
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCRIPT = REPO / "ci/scripts/build_wheels.py"
TAG = "v9.9.9"  # a version that cannot collide with a real fixture
VER = "9.9.9"

# (target, is_windows, filename compound tag, expected WHEEL Tag lines)
CASES = [
    (
        "x86_64-unknown-linux-musl",
        False,
        "manylinux_2_17_x86_64.musllinux_1_2_x86_64",
        ["py3-none-manylinux_2_17_x86_64", "py3-none-musllinux_1_2_x86_64"],
    ),
    ("x86_64-pc-windows-msvc", True, "win_amd64", ["py3-none-win_amd64"]),
    ("aarch64-pc-windows-msvc", True, "win_arm64", ["py3-none-win_arm64"]),
]


def fake_binary(os_name: str, cpu: str, tail: bytes = b"") -> bytes:
    if os_name == "linux":
        head = b"\x7fELF\x02\x01\x01" + b"\x00" * 11 + struct.pack("<H", {"x64": 0x3E, "arm64": 0xB7}[cpu])
    elif os_name == "darwin":
        head = b"\xcf\xfa\xed\xfe" + struct.pack("<I", {"x64": 0x01000007, "arm64": 0x0100000C}[cpu])
    else:
        head = b"MZ" + b"\x00" * 0x3A + struct.pack("<I", 0x40)
        head += b"PE\0\0" + struct.pack("<H", {"x64": 0x8664, "arm64": 0xAA64}[cpu])
    return head + b"\x00" * (80 - len(head)) + tail


BINARIES = {
    "x86_64-unknown-linux-musl": fake_binary("linux", "x64", b"-linux-x64"),
    "x86_64-pc-windows-msvc": fake_binary("win32", "x64", b"-win-x64"),
    "aarch64-pc-windows-msvc": fake_binary("win32", "arm64", b"-win-arm64"),
}
LICENSES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.html")


def make_tarball(
    tarball_dir: Path,
    target: str,
    is_windows: bool,
    *,
    tag: str = TAG,
    binary: bytes | None = None,
    sidecar: bool = True,
    bin_relpath: str | None = None,
) -> Path:
    top = f"alint-{tag}-{target}"
    bin_leaf = "alint.exe" if is_windows else "alint"
    tarball = tarball_dir / f"alint-{tag}-{target}.tar.gz"
    payload = BINARIES[target] if binary is None else binary
    with tarfile.open(tarball, "w:gz") as tf:

        def add(name: str, data: bytes, mode: int) -> None:
            ti = tarfile.TarInfo(f"{top}/{name}")
            ti.size = len(data)
            ti.mode = mode
            tf.addfile(ti, io.BytesIO(data))

        add(bin_relpath or bin_leaf, payload, 0o755)
        for name in LICENSES:
            add(name, f"{name} text\n".encode(), 0o644)
        add("README.md", b"# alint\n\nLint repository **structure**.\n", 0o644)
        add("docs/ARCHITECTURE.md", b"arch\n", 0o644)  # non-payload member: must be ignored
    if sidecar:
        digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
        (tarball_dir / f"{tarball.name}.sha256").write_text(f"{digest}  {tarball.name}\n")
    return tarball


def run(tarball_dir: Path, out: Path) -> None:
    subprocess.run(
        [sys.executable, str(SCRIPT), "--tag", TAG, "--tarball-dir", str(tarball_dir),
         "--out", str(out), "--targets", *[c[0] for c in CASES]],
        check=True,
    )


def check_wheel(out: Path, target: str, is_windows: bool, compound: str, tag_lines: list) -> None:
    wheel = out / f"alint-{VER}-py3-none-{compound}.whl"
    assert wheel.exists(), f"missing {wheel.name}"
    bin_leaf = "alint.exe" if is_windows else "alint"
    with zipfile.ZipFile(wheel) as zf:
        names = zf.namelist()
        payload = f"alint-{VER}.data/scripts/{bin_leaf}"
        assert payload in names, f"{payload} not in {names}"
        assert zf.read(payload) == BINARIES[target], "payload not byte-identical to release binary"
        mode = (zf.getinfo(payload).external_attr >> 16) & 0o7777
        assert mode == 0o755, f"binary exec bit wrong: {oct(mode)}"
        wheel_meta = zf.read(f"alint-{VER}.dist-info/WHEEL").decode()
        assert "Root-Is-Purelib: false" in wheel_meta
        for line in tag_lines:
            assert f"Tag: {line}" in wheel_meta, f"missing {line} in WHEEL:\n{wheel_meta}"
        md = zf.read(f"alint-{VER}.dist-info/METADATA").decode()
        assert "Name: alint" in md and f"Version: {VER}" in md
        assert "License: MIT OR Apache-2.0" in md
        assert "Requires-Python: >=3.7" in md
        assert "Description-Content-Type: text/markdown" in md and "# alint" in md
        for name in LICENSES:
            assert f"alint-{VER}.dist-info/licenses/{name}" in names, f"missing bundled {name}"
        record = zf.read(f"alint-{VER}.dist-info/RECORD").decode()
        checked = 0
        for row in record.splitlines():
            path, h, size = row.rsplit(",", 2)
            if path.endswith("/RECORD"):
                assert h == "" and size == "", "RECORD self-row must have empty hash/size"
                continue
            data = zf.read(path)
            exp = "sha256=" + base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
            assert h == exp, f"RECORD hash mismatch: {path}"
            assert int(size) == len(data), f"RECORD size mismatch: {path}"
            checked += 1
        assert checked >= 4, "RECORD verified too few entries"
    print(f"  OK {wheel.name} (payload byte-identical, RECORD verified)")


def check_targets_match_release() -> None:
    """Every release target gets a wheel, and no wheel names a target the release lacks."""
    sys.path.insert(0, str(SCRIPT.parent))
    from build_wheels import TARGET_PLATFORM_TAGS

    release = (REPO / ".github/workflows/release.yml").read_text()
    built = set(re.findall(r"^\s+- target:\s*([A-Za-z0-9_-]+)\s*$", release, re.MULTILINE))
    assert built, "could not read release.yml build-matrix targets"
    assert set(TARGET_PLATFORM_TAGS) == built, (
        f"build_wheels.TARGET_PLATFORM_TAGS {sorted(TARGET_PLATFORM_TAGS)} != release.yml targets {sorted(built)}"
    )
    print(f"  OK wheel targets match release.yml's {len(built)} build targets")


def main() -> int:
    if not SCRIPT.exists():
        print(f"[test-build-wheels] {SCRIPT} not found", file=sys.stderr)
        return 1
    check_targets_match_release()
    with tempfile.TemporaryDirectory() as td:
        tdp = Path(td)
        tarball_dir = tdp / "tarballs"
        tarball_dir.mkdir()
        for target, is_win, _, _ in CASES:
            make_tarball(tarball_dir, target, is_win)
        out1, out2 = tdp / "d1", tdp / "d2"
        run(tarball_dir, out1)
        for target, is_win, compound, tags in CASES:
            check_wheel(out1, target, is_win, compound, tags)
        run(tarball_dir, out2)
        for _, _, compound, _ in CASES:
            a = (out1 / f"alint-{VER}-py3-none-{compound}.whl").read_bytes()
            b = (out2 / f"alint-{VER}-py3-none-{compound}.whl").read_bytes()
            assert a == b, f"non-deterministic wheel: {compound}"
        print("  OK determinism: rebuilds are byte-identical")

        # Fail-closed hardening tests (H4):
        def run_one(tb_dir: Path, out_dir: Path, target: str, extra: list[str] = ()) -> subprocess.CompletedProcess:
            return subprocess.run(
                [sys.executable, str(SCRIPT), "--tag", TAG, "--tarball-dir", str(tb_dir),
                 "--out", str(out_dir), "--targets", target, *extra],
                capture_output=True, text=True,
            )

        # 1. Missing .sha256 sidecar must fail closed unless --allow-unsigned is passed.
        t_nosha = tdp / "nosha"
        t_nosha.mkdir()
        make_tarball(t_nosha, "x86_64-unknown-linux-musl", False, sidecar=False)
        r = run_one(t_nosha, tdp / "nosha-out", "x86_64-unknown-linux-musl")
        assert r.returncode != 0 and "no .sha256 sidecar" in r.stderr, r.stderr
        r_ok = run_one(t_nosha, tdp / "nosha-ok", "x86_64-unknown-linux-musl", ["--allow-unsigned"])
        assert r_ok.returncode == 0, r_ok.stderr
        print("  OK refuses missing .sha256 sidecar (unless --allow-unsigned)")

        # 2. Tag-mismatched tarball is rejected.
        t_tag = tdp / "wrongtag"
        t_tag.mkdir()
        make_tarball(t_tag, "x86_64-unknown-linux-musl", False, tag="v0.0.1")
        r = run_one(t_tag, tdp / "wrongtag-out", "x86_64-unknown-linux-musl")
        assert r.returncode != 0 and "no release tarball" in r.stderr, r.stderr
        print("  OK refuses tag-mismatched tarball")

        # 3. Wrong-arch binary header is rejected.
        t_arch = tdp / "wrongarch"
        t_arch.mkdir()
        make_tarball(t_arch, "x86_64-unknown-linux-musl", False, binary=fake_binary("linux", "arm64"))
        r = run_one(t_arch, tdp / "wrongarch-out", "x86_64-unknown-linux-musl")
        assert r.returncode != 0 and "binary header is" in r.stderr, r.stderr
        print("  OK refuses mismatched binary architecture header")

        # 4. Nested decoy `sub/alint` without top-level binary is rejected.
        t_decoy = tdp / "decoy"
        t_decoy.mkdir()
        make_tarball(t_decoy, "x86_64-unknown-linux-musl", False, bin_relpath="sub/alint")
        r = run_one(t_decoy, tdp / "decoy-out", "x86_64-unknown-linux-musl")
        assert r.returncode != 0 and "found inside the archive" in r.stderr, r.stderr
        print("  OK refuses nested decoy archive member")
    print("[test-build-wheels] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
