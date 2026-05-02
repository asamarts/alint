#!/usr/bin/env python3
"""Consolidate per-version bench results into a per-scenario lookup.

Reads `docs/benchmarks/macro/results/<arch>/<version>/results.json` for
every published version under `<arch>` and emits a flat dict suitable
for templating into HISTORY.md.

Usage:
    python3 xtask/scripts/consolidate-bench.py [--arch linux-x86_64]

Output: a Python `dict` literal printed to stdout, keyed by
`(version, scenario, size, mode)` → `(mean_ms, stddev_ms)`.

Designed to be sourced by the HISTORY.md update step (the actual
markdown generation is in `xtask/scripts/render-history.py` so a
maintainer can review the diff before merging the bench-record
PR — see RELEASING.md).
"""
import argparse
import glob
import json
import os
import sys
from typing import Dict, Tuple

Cell = Tuple[str, str, str, str]    # (version, scenario, size, mode)
Stat = Tuple[float, float]           # (mean_ms, stddev_ms)


def load_arch(base: str, arch: str) -> Dict[Cell, Stat]:
    """Walk `base/<arch>/v*/results.json` and parse into the lookup."""
    data: Dict[Cell, Stat] = {}
    arch_dir = os.path.join(base, arch)
    if not os.path.isdir(arch_dir):
        print(f"warning: {arch_dir} missing; nothing to load", file=sys.stderr)
        return data
    for vdir in sorted(os.listdir(arch_dir)):
        vpath = os.path.join(arch_dir, vdir)
        if not os.path.isdir(vpath):
            continue
        # Some versions split S8-only into a sibling subdir (s8full/);
        # walk recursively to pick up both `results.json` files.
        for rj in glob.glob(os.path.join(vpath, "**", "results.json"), recursive=True):
            with open(rj) as f:
                blob = json.load(f)
            for r in blob.get("rows", []):
                key = (vdir, r["scenario"], r["size_label"], r["mode"])
                data[key] = (r["mean_ms"], r["stddev_ms"])
    return data


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--arch", default="linux-x86_64")
    p.add_argument(
        "--base",
        default=os.path.join(
            os.path.dirname(os.path.dirname(os.path.dirname(__file__))),
            "docs", "benchmarks", "macro", "results",
        ),
    )
    args = p.parse_args()

    data = load_arch(args.base, args.arch)
    if not data:
        return 1

    print("# (version, scenario, size, mode) -> (mean_ms, stddev_ms)")
    print("DATA = {")
    for k in sorted(data):
        v = data[k]
        print(f"    {k!r}: ({v[0]:.4f}, {v[1]:.4f}),")
    print("}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
