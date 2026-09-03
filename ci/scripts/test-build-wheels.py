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
]
BINARIES = {
    "x86_64-unknown-linux-musl": b"\x7fELF-fake-linux-" + b"\x00" * 61,
    "x86_64-pc-windows-msvc": b"MZ-fake-windows-" + b"\x00" * 64,
}
LICENSES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "THIRD-PARTY-LICENSES.html")


def make_tarball(tarball_dir: Path, target: str, is_windows: bool) -> None:
    top = f"alint-{TAG}-{target}"
    bin_leaf = "alint.exe" if is_windows else "alint"
    tarball = tarball_dir / f"alint-{TAG}-{target}.tar.gz"
    with tarfile.open(tarball, "w:gz") as tf:

        def add(name: str, data: bytes, mode: int) -> None:
            ti = tarfile.TarInfo(f"{top}/{name}")
            ti.size = len(data)
            ti.mode = mode
            tf.addfile(ti, io.BytesIO(data))

        add(bin_leaf, BINARIES[target], 0o755)
        for name in LICENSES:
            add(name, f"{name} text\n".encode(), 0o644)
        add("README.md", b"# alint\n\nLint repository **structure**.\n", 0o644)
        add("docs/ARCHITECTURE.md", b"arch\n", 0o644)  # non-payload member: must be ignored
    digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
    (tarball_dir / f"{tarball.name}.sha256").write_text(f"{digest}  {tarball.name}\n")


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


def main() -> int:
    if not SCRIPT.exists():
        print(f"[test-build-wheels] {SCRIPT} not found", file=sys.stderr)
        return 1
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
    print("[test-build-wheels] PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
