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
from typing import Dict, Iterable, List, Tuple

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
                # Sentinel so we don't accidentally start a new
                # paragraph for the same version.
                cur_version_locked = cur_version
                cur_version = None
                cur_date = None
                # Re-arm `cur_version` only when we hit the next
                # `##` header. To do that we need to NOT reset to
                # None — actually we DO reset, the next `##` sets
                # it again. The `_locked` var is unused; left for
                # the reader to grok the intent.
                _ = cur_version_locked
                continue
            cur_para.append(stripped)

    flush()
    return out

SIZES = ["1k", "10k", "100k", "1m"]
MODES = ["full", "changed"]

# (id, title, intro)
SCENARIOS = [
    (
        "S1", "Filename hygiene",
        "Eight filename-only rules (`filename_case`, `filename_regex`). Pure walker plus glob match — no content read. Narrowest scope alint shares with `ls-lint`, used as the competitive-comparison anchor. Catches walker and scope-match regressions.",
    ),
    (
        "S2", "Existence + content",
        "Eight existence + content rules (`file_exists`, `file_absent`, `file_content_forbidden`, `file_max_size`). Walker plus per-file content scan over narrow scopes. Repolinter-comparable shape. Catches content-rule regressions on common shapes.",
    ),
    (
        "S3", "Workspace bundle",
        "`extends: oss-baseline + rust + monorepo + cargo-workspace` (~34 rules). Heavy mix — content rules over `**/*.rs`, cross-file `for_each_dir` over `crates/*`, `toml_path_matches` per crate. Realistic monorepo workload; the v0.9.5 cliff (`investigations/2026-05-cross-file-rules/`) lived here.",
    ),
    (
        "S4", "Agent-era hygiene",
        "Five rules from the v0.6 `agent-hygiene` bundled ruleset (`file_absent`, `file_content_forbidden`). Filename plus content fan-out over agent-shaped trees. Catches agent-era rule shapes.",
    ),
    (
        "S5", "Fix-pass content edits",
        "Four content-edit rules under `--fix` (`final_newline`, `no_trailing_whitespace`, `line_endings`, `no_bom`). Read, transform, atomic-rename. The only `--fix`-mode bench. Catches fix-pipeline regressions.",
    ),
    (
        "S6", "Per-file content fan-out",
        "Thirteen content rules over `**/*.rs`. Per-file dispatch path width — every `.rs` file hit by every rule on a single read. Stresses the v0.9.3 dispatch-flip read-coalescing path. Catches per-file inner-loop regressions S3 doesn't surface.",
    ),
    (
        "S7", "Cross-file relational",
        "Six cross-file relational kinds (`pair`, `unique_by`, `for_each_dir`, `for_each_file`, `dir_only_contains`, `every_matching_has`). Various fan-out shapes over the synthetic monorepo. Catches the next O(D × N) cliff after the v0.9.5 path-index fix; the v0.9.7 → v0.9.8 transition's headline cell.",
    ),
    (
        "S8", "Git overlay",
        "S3 reshape plus `git_no_denied_paths` and `git_tracked_only` over a real git repo. Same as S3 but with `Engine::collect_git_tracked_if_needed` and `BlameCache` active. Catches git-aware dispatch regressions at scale.",
    ),
    (
        "S9", "Nested polyglot",
        "Three competing ecosystem rulesets: `extends: rust + node + python` (~26 rules) over a polyglot tree (Rust under `crates/`, Node under `packages/`, Python under `apps/`). Per-rule `scope_filter: { has_ancestor: <manifest> }` ancestor walks. The dispatch shape the v0.9.6 `scope_filter:` primitive was designed for — without it, every `**/*.py` rule from python@v1 fires on every `.py` file in the tree. **New in v0.9.6.**",
    ),
    (
        "S10", "scope_filter outside per-file dispatch",
        "Five rules from outside the `PerFileRule` dispatch path (`file_max_size`, `no_empty_files`, `no_symlinks`, `filename_case`, `filename_regex`) each with `scope_filter: { has_ancestor: <manifest> }` over the polyglot tree. Per-rule `evaluate()` iterating `ctx.index.files()` with both path-glob AND scope_filter narrowing — the dispatch shape v0.9.9 wired through (v0.9.8 silently dropped `scope_filter:` on these 17 rule kinds). **New in v0.9.9.**",
    ),
]

# Manual cells from the published v0.5.6 markdown (no JSON exists).
MANUAL = {
    ("v0.5.6", "S3", "1m", "full"):    (569078.0, 60911.0),
    ("v0.5.6", "S3", "1m", "changed"): (528103.0, 2537.0),
}


def load_arch(base: str, arch: str) -> Dict[Cell, Stat]:
    data = dict(MANUAL)
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
        # S9 didn't exist before v0.9.6.
        if s == "S9" and v in ("v0.9.5", "v0.9.4", "v0.5.7", "v0.5.6"):
            return "n/a"
        # S10 didn't exist before v0.9.9.
        if s == "S10" and v in ("v0.9.8", "v0.9.7", "v0.9.6", "v0.9.5", "v0.9.4", "v0.5.7", "v0.5.6"):
            return "n/a"
        return "—"
    mean, sd = cell
    if mean < 1000:
        return f"{mean:.0f} ms ± {sd:.0f}"
    if mean < 60000:
        return f"{mean/1000:.2f} s ± {sd/1000:.2f}"
    return f"{mean/1000:.1f} s ± {sd/1000:.1f}"


def render(
    data: Dict[Cell, Stat],
    changelog_headlines: Dict[str, Tuple[str, str]] | None = None,
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
    out: list[str] = []
    out += [
        "# alint perf history",
        "",
        "Per-scenario tables, version-trajectory shape. Headline cells fingerprinted",
        "to `linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95) —",
        "see [`METHODOLOGY.md`](METHODOLOGY.md) for the hardware contract and why",
        "cross-machine comparisons need like-for-like.",
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
        "1M cells across the most-stressed scenarios. S3 is the realistic-monorepo",
        "anchor; S7 is the cross-file-relational cliff that v0.9.5's path-index fix",
        "didn't fully cover (and v0.9.8 targets directly); S9 is the nested-polyglot",
        "scenario the v0.9.6 `scope_filter:` primitive exists for.",
        "",
        "| Version | Date | 1M S3 full | 1M S6 full | 1M S7 full | 1M S9 full | Headline change |",
        "|---|---|---:|---:|---:|---:|---|",
    ]
    # Date table — one row per version present on disk.
    # Manual fallbacks for versions older than CHANGELOG.md
    # carries (or for one-off bench-only entries like v0.5.6/.7).
    # CHANGELOG-parsed headlines win when both are defined, so
    # new releases need no script edit.
    fallback_headlines = {
        "v0.5.7": ("2026-03", "First publish-grade `bench-scale` matrix at 1k/10k/100k."),
        "v0.5.6": ("2026-03", "Prep run that captured the only pre-v0.9 1M S3 numbers."),
    }
    headlines = dict(fallback_headlines)
    if changelog_headlines:
        headlines.update(changelog_headlines)
    for v in versions_present:
        date, headline = headlines.get(v, ("?", "—"))
        cells = [fmt(data, v, sx, "1m", "full") for sx in ("S3", "S6", "S7", "S9")]
        marker = "**" if v == versions_present[0] else ""
        out.append(f"| {marker}{v}{marker} | {date} | {' | '.join(cells)} | {headline} |")
    out += [
        "",
        "Earlier history (v0.7.x, v0.8.x): no measured perf change beyond v0.5.7;",
        "see [CHANGELOG.md](../../CHANGELOG.md) for the contemporaneous notes.",
        "",
        "---",
        "",
    ]
    # Per-scenario sections
    for sid, title, intro in SCENARIOS:
        out += [f"## {sid} — {title}", "", intro, ""]
        for mode in MODES:
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
        "When a release tag lands, the `bench-record.yml` workflow (introduced in",
        "v0.9.7) auto-runs the publish-grade matrix on the self-hosted Linux runner",
        "and opens a PR with the new per-version dir. The maintainer re-renders this",
        "file from the merged data:",
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
        "- v0.9.5 cliff (S3 1M): [`investigations/2026-05-cross-file-rules/`](investigations/2026-05-cross-file-rules/)",
        "  — surfaced the +28-37 % regression vs v0.5.6 and the lazy-path-index fix.",
        "- v0.9.5 → v0.9.8 cliff (S7 1M): [`investigations/2026-05-cross-file-rules-v2/`](investigations/2026-05-cross-file-rules-v2/)",
        "  — surfaced the residual O(D × N) shape in `dir_only_contains` /",
        "  `dir_contains` after the v0.9.5 fix; v0.9.8 closes it via",
        "  `FileIndex::children_of`. *(Investigation written alongside v0.9.8.)*",
    ]
    return "\n".join(out) + "\n"


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
    args = p.parse_args()

    data = load_arch(args.base, args.arch)
    changelog_headlines = parse_changelog(args.changelog)
    if not data:
        return 1
    sys.stdout.write(render(data, changelog_headlines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
