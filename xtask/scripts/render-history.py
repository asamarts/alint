#!/usr/bin/env python3
"""Render HISTORY.md from per-version bench JSONs.

Reads `docs/benchmarks/macro/results/<arch>/<version>/results.json`
for every published version under `<arch>` and emits a complete
HISTORY.md with per-scenario tables populated.

v0.9.14 made this script auto-discovering: versions are read from
the filesystem (no hardcoded `KNOWN_VERSIONS` list to keep in sync
with releases), and the per-version date + headline blurb in the
cross-version trajectory table are extracted from `CHANGELOG.md`'s
`## [X.Y.Z] — YYYY-MM-DD` headers + the first paragraph beneath
each. The `bench-record.yml` workflow runs this script
automatically before opening its PR so HISTORY.md never falls out
of date with the published bench corpus.

This renders the CONSOLIDATED 5-scenario series (S1 layout / S2
content / S3 relational-graph / S4 workspace, plus the fix-mode
SFIX scenario which has no published data until the v0.17 fix
engine ships). The pre-consolidation 14-scenario kbench series is
FROZEN at `docs/benchmarks/legacy/` and rendered from
`docs/benchmarks/macro/results/legacy/` with this script's
git-history predecessor; it is not re-rendered.

Usage:
    python3 xtask/scripts/render-history.py [--arch linux-x86_64] \
        [--changelog CHANGELOG.md] \
        > docs/benchmarks/HISTORY.md

Then `git diff docs/benchmarks/HISTORY.md` for review.
"""
import argparse
import glob
import json
import os
import re
import sys
from typing import Dict, List, Tuple

Cell = Tuple[str, str, str, str]    # (version, scenario, size, mode)
Stat = Tuple[float, float]           # (mean_ms, stddev_ms)


def semver_key(v: str) -> Tuple[int, ...]:
    """Sort key for `vX.Y.Z` strings — newer is greater."""
    parts = v.lstrip("v").split(".")
    out: List[int] = []
    for p in parts:
        try:
            out.append(int(p))
        except ValueError:
            # Non-numeric suffix (rc, etc.) — treat as 0.
            out.append(0)
    return tuple(out)


def discover_versions(arch_dir: str) -> List[str]:
    """Versions present on disk, sorted newest-first.

    Replaces the pre-v0.9.14 hardcoded `KNOWN_VERSIONS` list. New
    releases land their `vX.Y.Z/` dir via the `bench-record.yml`
    workflow; this script picks them up without code changes.
    """
    if not os.path.isdir(arch_dir):
        return []
    versions = [
        d
        for d in os.listdir(arch_dir)
        if d.startswith("v") and os.path.isdir(os.path.join(arch_dir, d))
    ]
    return sorted(versions, key=semver_key, reverse=True)


def first_sentence(text: str) -> str:
    """Return the first sentence of `text` (everything up to the
    first `. ` or `.<EOF>`). Conservative — a sentence ends only
    on `. `, not on `e.g.`/`i.e.`/version numbers. Intended for
    extracting one-row table headlines from CHANGELOG paragraphs.
    """
    # Match "<sentence>. " followed by a capital letter,
    # backtick (code span), or `vN.M.…` (version reference —
    # CHANGELOG entries frequently start follow-up sentences
    # with a `vX.Y.Z` reference). Falls back to returning
    # `text` if no boundary is found.
    m = re.search(r"\.\s+(?=[A-Z`]|v[0-9])", text)
    if m:
        return text[: m.start() + 1]
    # No mid-string boundary; if the text ends with `.`, return
    # as-is. Otherwise return the whole thing (callers may want
    # to truncate further).
    return text


def parse_changelog(path: str) -> Dict[str, Tuple[str, str]]:
    """Extract `{version: (date, first_paragraph)}` from a Keep-a-
    Changelog-flavoured CHANGELOG.md.

    Looks for `## [X.Y.Z] — YYYY-MM-DD` (em dash or hyphen-minus)
    headers and grabs the first non-empty paragraph that follows
    each, stopping at the next `##`/`###` header or blank line.
    Returns an empty dict if the file is missing — callers fall
    back to `(?, —)`.
    """
    out: Dict[str, Tuple[str, str]] = {}
    if not os.path.isfile(path):
        return out
    with open(path) as f:
        lines = f.readlines()

    # Match either an em dash (—, U+2014) or a hyphen-minus.
    header_re = re.compile(r"^##\s+\[([0-9A-Za-z.\-]+)\]\s+[—-]\s+(\d{4}-\d{2}-\d{2})")
    cur_version = None
    cur_date = None
    cur_para: List[str] = []
    in_para = False

    def flush() -> None:
        if cur_version and cur_para:
            full_para = " ".join(cur_para).strip()
            # Extract the first sentence — the CHANGELOG's full
            # opening paragraph is too long for a one-row table
            # cell. We take everything up to the first `. ` (or
            # the end of the paragraph). Maintainers writing
            # CHANGELOG entries should make the first sentence a
            # punchy one-liner; the rest of the paragraph
            # remains as the long-form blurb in CHANGELOG itself.
            blurb = first_sentence(full_para)
            out[f"v{cur_version}"] = (cur_date, blurb)

    for line in lines:
        m = header_re.match(line)
        if m:
            flush()
            cur_version = m.group(1)
            cur_date = m.group(2)
            cur_para = []
            in_para = False
            continue
        if cur_version is None:
            continue
        stripped = line.strip()
        # Sub-headers (### Foo) or the next top-level (## ...) end
        # the headline paragraph. We don't `flush()` here because
        # the next iteration's header line will (or EOF will).
        if stripped.startswith("###") or (
            stripped.startswith("## ") and not header_re.match(line)
        ):
            flush()
            cur_version = None
            cur_date = None
            cur_para = []
            in_para = False
            continue
        if not in_para:
            if not stripped:
                continue
            in_para = True
            cur_para.append(stripped)
        else:
            if not stripped:
                # Blank line ends the paragraph; subsequent lines
                # within the same version section don't get added
                # back (only the first paragraph is the headline).
                flush()
                cur_para = []
                in_para = False
                cur_version = None
                cur_date = None
                continue
            cur_para.append(stripped)

    flush()
    return out

SIZES = ["1k", "10k", "100k", "1m"]
MODES = ["full", "changed"]


def modes_for(sid: str) -> List[str]:
    """SFIX runs `fix` mode only; the check scenarios run full + changed."""
    return ["fix"] if sid == "SFIX" else MODES

# (id, title, intro) — the CONSOLIDATED 5-scenario set (see
# docs/design/bench/scenario-consolidation.md). Each new scenario
# absorbs several of the old 14; the pre-consolidation series is
# frozen at docs/benchmarks/legacy/.
#
# SFIX is the dedicated fix-mode scenario: it runs `fix` mode (not
# `full`/`changed`), so render() gives it a fix-mode table via
# modes_for(). It has no published data until the v0.17 fix engine
# ships, so FIRST_VERSION gates it to `n/a` for every earlier tag.
# It is INCLUDED here (not deferred) so bench-record's render-history
# does not hard-fail the recurrence guard -- and truncate HISTORY.md
# via `> HISTORY.md` -- the first time a real SFIX row lands.
SCENARIOS = [
    (
        "S1", "Layout & path",
        "Walker + `GlobSet` + path/metadata rules with little or no content read — the cheapest dispatch path and the `ls-lint` / `grep` competitive anchor. Consolidates the old filename-hygiene, `scope_filter`-shape, and existence/size scenarios; a subset carries `scope_filter: { has_ancestor }` so the non-per-file-rule scope_filter dispatch shape stays covered. Catches walker / glob / scope-match regressions.",
    ),
    (
        "S2", "Per-file content",
        "Per-file dispatch fan-out: every `**/*.rs` file is read once and hit by the whole content mix — per-line kinds, forbidden-pattern scans, `ordered_block` / `import_gate`, and structured XML against a single `.csproj` overlay. Consolidates the old dense content-fan-out and v0.10 per-file scenarios. The perf signal is how flat the per-rule cost stays as the mix grows.",
    ),
    (
        "S3", "Cross-file, relational & graph",
        "Whole-index build + path-index + relational fan-out (`pair` / `unique_by` / `for_each_*` / `dir_only_contains` / `every_matching_has`) + `file_graph` build/traversal ×3 + `for_each_match` + `cross_file` set-union + single-shot spawn (`command: [\"true\"]`). Consolidates the old cross-file, v0.10 cross-file, single-shot, and v0.12 graph/featureset scenarios. The heaviest scenario; the index/graph build is the perf signal, so `changed` mode is ~as costly as `full`. **New at v0.12.0** (the graph kinds land at v0.12).",
    ),
    (
        "S4", "Workspace bundle",
        "The realistic mixed workload and the release anchor: `extends:` six bundled rulesets (oss-baseline + rust + node + python + monorepo + cargo-workspace) over a POLYGLOT + GIT tree (`crates/` + `packages/` + `apps/`, initialised as a real repo) with `nested_configs` on and two inline git-aware rules. Consolidates the old realistic-monorepo, git-overlay, and nested-polyglot scenarios. The `s4_1m_full` cell is the trajectory anchor every publish captures.",
    ),
    (
        "SFIX", "Auto-fix",
        "The fix engine over all 24 non-spawning fix ops (`FixSpec::ALL_OP_NAMES` minus `command` / `git_untrack`), run as `alint fix --unsafe-fixes --dry-run` so every op's compute + compose path runs while nothing is written. The only fix-mode scenario -- closes the historical zero-macro-fix-coverage gap. Rendered as a `fix`-mode table (not `full`/`changed`); `n/a` for every tag before the v0.17 fix engine. **New at v0.17.0.**",
    ),
]

# Per-scenario "this is the first version where the scenario exists".
# Older versions render `n/a` (vs `—` which means "version exists but
# wasn't measured at that size"). Extend when adding a scenario; no
# need to enumerate every prior tag. The consolidated series starts at
# v0.10.0 (the kbench corpus floor); S1/S2/S4 exist across all of it,
# S3's graph kinds land at v0.12.0.
FIRST_VERSION: Dict[str, str] = {
    "S3": "v0.12.0",
    # SFIX first ships with the v0.17 fix engine; no earlier tag has a
    # fix-mode row, so every pre-v0.17 SFIX cell renders `n/a`.
    "SFIX": "v0.17.0",
}

# Manual pre-results.json cells belong only to the retired 3900X series
# (`linux-x86_64-ryzen-3900x`); the canonical kbench series is measured.
MANUAL: Dict[Cell, Stat] = {}

# Host fingerprint per published arch series, for the HISTORY header line.
# `linux-x86_64` is the canonical kbench series (2026-07 onward); the retired
# 3900X dev-box series lives at `linux-x86_64-ryzen-3900x` (alint.org/benchmarks-1).
FINGERPRINT = {
    "linux-x86_64": "Intel Core i7-6700HQ 4-core / 15 GB / ext4 / rustc 1.97.0",
    "linux-x86_64-ryzen-3900x": "AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95",
}


def load_arch(base: str, arch: str) -> Dict[Cell, Stat]:
    data = dict(MANUAL) if arch == "linux-x86_64-ryzen-3900x" else {}
    arch_dir = os.path.join(base, arch)
    if not os.path.isdir(arch_dir):
        print(f"warning: {arch_dir} missing; nothing to load", file=sys.stderr)
        return data
    for vdir in sorted(os.listdir(arch_dir)):
        vpath = os.path.join(arch_dir, vdir)
        if not os.path.isdir(vpath):
            continue
        for rj in glob.glob(os.path.join(vpath, "**", "results.json"), recursive=True):
            with open(rj) as f:
                blob = json.load(f)
            for r in blob.get("rows", []):
                key = (vdir, r["scenario"], r["size_label"], r["mode"])
                data[key] = (r["mean_ms"], r["stddev_ms"])
    return data


def fmt(data: Dict[Cell, Stat], v: str, s: str, sz: str, m: str) -> str:
    cell = data.get((v, s, sz, m))
    if cell is None:
        # Scenarios that don't exist at the rendered tag get `n/a`;
        # `—` is reserved for "version exists, size not measured".
        first = FIRST_VERSION.get(s)
        if first is not None and semver_key(v) < semver_key(first):
            return "n/a"
        return "—"
    mean, sd = cell
    if mean < 1000:
        return f"{mean:.0f} ms ± {sd:.0f}"
    if mean < 60000:
        return f"{mean/1000:.2f} s ± {sd/1000:.2f}"
    return f"{mean/1000:.1f} s ± {sd/1000:.1f}"


# The four headline trajectory cells: 1M/full for each check scenario.
# json_key = the scenario label lowercased (the harness writes
# scenario="S1".."S4" via Scenario::label(); render keys on that).
HEADLINE_CELLS = [
    ("s1_1m_full", "S1"),
    ("s2_1m_full", "S2"),
    ("s3_1m_full", "S3"),
    ("s4_1m_full", "S4"),
]


def render(
    data: Dict[Cell, Stat],
    changelog_headlines: Dict[str, Tuple[str, str]] | None = None,
    arch: str = "linux-x86_64",
) -> str:
    """Produce the full HISTORY.md text. Caller redirects to file.

    `changelog_headlines` (when provided) is the parsed
    `{version: (date, blurb)}` map from CHANGELOG.md; values
    override the embedded `headlines` dict for any version that
    appears in both. New releases land their CHANGELOG entry,
    and the corresponding HISTORY row picks up date+blurb
    automatically — no edit to this script required.
    """
    versions_present = sorted({k[0] for k in data}, key=semver_key, reverse=True)

    # Recurrence guard (added after S14 was silently dropped): every scenario
    # measured in the corpus MUST have a SCENARIOS entry, else its section is
    # skipped and HISTORY.md under-reports the matrix while nothing complains.
    # Fail loudly so bench-record.yml's re-render surfaces a new scenario that
    # needs a hand-written intro here. (A future SFIX/fix-mode row trips this
    # on purpose — see the SCENARIOS note.)
    known = {sid for sid, _title, _intro in SCENARIOS}
    measured = {k[1] for k in data}
    missing = sorted(measured - known, key=lambda s: int(s[1:]) if s[1:].isdigit() else 0)
    if missing:
        sys.stderr.write(
            f"render-history: {len(missing)} benched scenario(s) missing from the "
            f"SCENARIOS list, so they would be dropped from HISTORY.md: "
            f"{', '.join(missing)}. Add each to SCENARIOS (+ FIRST_VERSION).\n"
        )
        sys.exit(1)

    out: list[str] = []
    out += [
        "# alint perf history",
        "",
        "Per-scenario tables, version-trajectory shape. Headline cells fingerprinted",
        f"to `{arch}` ({FINGERPRINT.get(arch, 'see METHODOLOGY.md')}) —",
        "see [`METHODOLOGY.md`](METHODOLOGY.md) for the hardware contract and why",
        "cross-machine comparisons need like-for-like.",
        "",
        "This is the consolidated 5-scenario series (S1 layout / S2 content /",
        "S3 relational-graph / S4 workspace; the fix-mode SFIX scenario has no",
        "published data until the v0.17 fix engine ships). The pre-consolidation",
        "14-scenario history is frozen at [`legacy/HISTORY.md`](legacy/HISTORY.md).",
        "",
        "## How to read this file",
        "",
        "Each scenario gets its own section with:",
        "",
        "1. A one-paragraph overview of what dispatch shape the scenario stresses",
        "   and which class of regression it catches.",
        "2. A table per mode (`full` and `changed`) with rows = version (newest",
        "   first), columns = size (1k / 10k / 100k / 1M). Cells are",
        "   `mean ± stddev`, formatted in ms below 1 s and seconds above.",
        "3. `—` means the version was not measured at that size.",
        "   `n/a` means the scenario didn't exist at the tag.",
        "",
        "Significant deltas (anything > 20 % across a release) get an investigation",
        "write-up under [`investigations/<YYYY-MM-topic>/`](investigations/) that",
        "captures the diagnostic data (traces, flamegraphs, bisect notes).",
        "",
        "**Source of truth.** This file is generated by",
        "`xtask/scripts/render-history.py` after every release. The bench-record.yml",
        "workflow's PR includes the per-cell numbers for the maintainer to verify",
        "before merging — see [`../../RELEASING.md`](../../RELEASING.md).",
        "",
        "## Cross-version headline trajectory",
        "",
        "1M/full cells across all four check scenarios. S4 (workspace bundle) is the",
        "realistic-monorepo release anchor every publish captures; S3 (cross-file,",
        "relational & graph) is the heaviest, graph-dominated scenario; S1/S2 track",
        "the walk-bound and per-file-content paths.",
        "",
        "| Version | Date | 1M S1 full | 1M S2 full | 1M S3 full | 1M S4 full | Headline change |",
        "|---|---|---:|---:|---:|---:|---|",
    ]
    # Date table — one row per version present on disk. CHANGELOG-parsed
    # headlines win when defined, so new releases need no script edit.
    headlines: Dict[str, Tuple[str, str]] = {}
    if changelog_headlines:
        headlines.update(changelog_headlines)
    for v in versions_present:
        date, headline = headlines.get(v, ("?", "—"))
        cells = [fmt(data, v, sx, "1m", "full") for sx in ("S1", "S2", "S3", "S4")]
        marker = "**" if v == versions_present[0] else ""
        out.append(f"| {marker}{v}{marker} | {date} | {' | '.join(cells)} | {headline} |")
    out += [
        "",
        "Pre-consolidation history (the 14-scenario kbench series v0.10.0-v0.16.0, and",
        "the retired 3900X series before it) lives in",
        "[`legacy/HISTORY.md`](legacy/HISTORY.md) and on",
        "[alint.org/benchmarks-1](https://alint.org/benchmarks-1/).",
        "",
        "---",
        "",
    ]
    # Per-scenario sections
    for sid, title, intro in SCENARIOS:
        out += [f"## {sid} — {title}", "", intro, ""]
        for mode in modes_for(sid):
            out += [
                f"### {sid} — {mode}",
                "",
                "| Version | 1k | 10k | 100k | 1M |",
                "|---|---:|---:|---:|---:|",
            ]
            for v in versions_present:
                row_cells = [fmt(data, v, sid, sz, mode) for sz in SIZES]
                marker = "**" if v == versions_present[0] else ""
                out.append(f"| {marker}{v}{marker} | {' | '.join(row_cells)} |")
            out.append("")
    out += [
        "## How to add a row",
        "",
        "When a release tag lands, the `bench-record.yml` workflow auto-runs the",
        "publish-grade matrix on the self-hosted Linux runner and opens a PR with the",
        "new per-version dir. The maintainer re-renders this file from the merged data:",
        "",
        "```sh",
        "python3 xtask/scripts/render-history.py > docs/benchmarks/HISTORY.md",
        "```",
        "",
        "See [`../../RELEASING.md`](../../RELEASING.md) for the full review checklist",
        "(CV check, fingerprint check, investigation hand-off if delta > 20 %).",
        "",
        "## Cross-version perf investigations",
        "",
        "Pre-consolidation cliff investigations (v0.9.5 cross-file, v0.9.8 O(D × N),",
        "the 1M writeback-contention artifact) are catalogued against the frozen",
        "14-scenario series in [`legacy/HISTORY.md`](legacy/HISTORY.md); the raw",
        "diagnostic data stays under [`investigations/`](investigations/).",
    ]
    return "\n".join(out) + "\n"


def render_trajectory_json(
    data: Dict[Cell, Stat],
    changelog_headlines: Dict[str, Tuple[str, str]] | None,
    arch: str,
) -> str:
    """Produce a machine-readable trajectory.json mirroring the
    cross-version headline table at the top of HISTORY.md.

    Consumed by alint.org's `/benchmarks/` page (see
    `src/pages/benchmarks.astro` over in the site repo) so the
    trajectory table refreshes automatically on every main push
    instead of drifting until a maintainer hand-edits HTML rows.
    Schema is `schema_version: 2` (v2 = consolidated 5-scenario
    series with `s1..s4_1m_full` cell keys; v1 was the frozen
    14-scenario series with `s3/s6/s7/s9_1m_full`). Field additions
    are non-breaking; removals/semantic changes bump the version.

    The four cell columns mirror the markdown table: 1M S1/S2/S3/S4
    in `full` mode. Consumers that want the full matrix render
    HISTORY.md directly.
    """
    headlines: Dict[str, Tuple[str, str]] = {}
    if changelog_headlines:
        headlines.update(changelog_headlines)

    versions_present = sorted({k[0] for k in data}, key=semver_key, reverse=True)
    rows = []
    for v in versions_present:
        date, headline = headlines.get(v, ("?", "—"))
        cells = {}
        for json_key, scenario in HEADLINE_CELLS:
            stat = data.get((v, scenario, "1m", "full"))
            if stat is None:
                # Scenarios that don't exist at this tag, AND scenarios
                # that exist but lack the 1M cell, both fall through to
                # `None` here. Mirror fmt()'s n/a-vs-— distinction via
                # FIRST_VERSION so the markdown table and this JSON
                # don't drift on those cells.
                cells[json_key] = None
                continue
            mean_ms, stddev_ms = stat
            cells[json_key] = {
                "mean_ms": mean_ms,
                "stddev_ms": stddev_ms,
                "display": fmt(data, v, scenario, "1m", "full"),
            }
        rows.append({
            "version": v,
            "date": date,
            "headline": headline,
            "cells": cells,
        })
    payload = {
        "schema_version": 2,
        "arch": arch,
        "rows": rows,
    }
    return json.dumps(payload, indent=2, ensure_ascii=False) + "\n"


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
    p.add_argument(
        "--changelog",
        default=os.path.join(
            os.path.dirname(os.path.dirname(os.path.dirname(__file__))),
            "CHANGELOG.md",
        ),
        help="CHANGELOG.md path; release date + headline blurb come from here",
    )
    p.add_argument(
        "--json-out",
        default=None,
        help=(
            "Optional: also write a JSON trajectory file to this path. "
            "Consumed by alint.org's /benchmarks/ page so the table doesn't "
            "drift. Stdout markdown is unaffected."
        ),
    )
    args = p.parse_args()

    # The markdown render contains em dashes (—) and other Unicode
    # characters. Windows' default stdout codepage is cp1252 and
    # would raise UnicodeEncodeError on `sys.stdout.write(...)`.
    # Force UTF-8 so the script works the same on Linux (CI / dev),
    # macOS (CI), and Windows (cross-platform CI).
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")

    data = load_arch(args.base, args.arch)
    changelog_headlines = parse_changelog(args.changelog)
    if not data:
        return 1
    sys.stdout.write(render(data, changelog_headlines, args.arch))
    if args.json_out:
        os.makedirs(os.path.dirname(os.path.abspath(args.json_out)) or ".", exist_ok=True)
        with open(args.json_out, "w", encoding="utf-8") as f:
            f.write(render_trajectory_json(data, changelog_headlines, args.arch))
    return 0


if __name__ == "__main__":
    sys.exit(main())
