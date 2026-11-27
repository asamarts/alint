#!/usr/bin/env python3
"""Fail when docs that alint.org publishes from main carry unmarked new lines.

docs-bundle.yml builds alint.org's docs from the latest release tag, then
overlays some pages from main and strips each `<!-- alint:since=X -->` block
newer than the release (strip-since.mjs). A line added on main outside such a
block therefore goes live before the release that ships what it says. This
check finds every line added since the latest `v*` tag in those pages and fails
when one sits outside a since-block.

Wrap new text in `<!-- alint:since=X -->` ... `<!-- /alint:since -->`, where X
is the release that ships what the text says: the next release for unreleased
behaviour, or a released version (the latest tag, say) for a correction of
behaviour that has already shipped. A released version's block is kept on the
site; the markers can go once the next release is tagged.

What it checks, all read from the working tree against the tag:
- every `.md` file under the paths docs-bundle.yml passes to strip-since.mjs,
  except the design documents that track main by design (ROADMAP.md and
  ARCHITECTURE.md);
- docs/rules.md, which the rule-page bridge renders from main: the sections of
  the rule kinds that existed at the tag, and the fix-operations and nested
  configuration sections. A new kind's section is withheld from the site with
  its page, so it is not checked.
It also refuses a since marker whose version does not parse, since the
stripper reads an unparsable version as 0.0.0 and keeps the block.

Exempt: blank lines; the release's Action pin (`uses: asamarts/alint@<sha> #
vX.Y.Z`), which docs-bundle.yml verifies against ci/action-doc-pin.env; and the
counts on the page pin-docs-counts.py renders from the tag's facts.json (the
check renders the page the same way before comparing).

Usage: check-docs-overlay-since.py [--repo DIR] [--base REF]
Exit 0 when clean, 1 on unmarked lines, 2 when it cannot run.
"""

from __future__ import annotations

import argparse
import difflib
import importlib.util
import re
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

WORKFLOW = ".github/workflows/docs-bundle.yml"
RULES_MD = "docs/rules.md"
# Living design documents that the site shows as main's (docs-bundle.yml's
# overlay comment): they describe plans and architecture, not shipped behaviour.
TRACKS_MAIN = {"docs/design/ROADMAP.md", "docs/design/ARCHITECTURE.md"}
SINCE_OPEN = "<!-- alint:since="
SINCE_CLOSE = "<!-- /alint:since -->"
VERSION = re.compile(r"^v?\d+(\.\d+){0,2}$")
ACTION_PIN = re.compile(r"^\s*(-\s+)?uses:\s*asamarts/alint@[0-9a-f]{40} # v\d+\.\d+\.\d+\s*$")


class SetupError(Exception):
    pass


def git(repo: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess:
    proc = subprocess.run(
        ["git", "-C", str(repo), *args], capture_output=True, text=True, check=False
    )
    if check and proc.returncode != 0:
        raise SetupError(f"git {' '.join(args)}: {proc.stderr.strip()}")
    return proc


def base_tag(repo: Path) -> str:
    proc = git(repo, "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", check=False)
    if proc.returncode != 0:
        raise SetupError(
            "no v* release tag is reachable from HEAD; fetch tags "
            "(actions/checkout with fetch-depth: 0) or pass --base"
        )
    return proc.stdout.strip()


def at_base(repo: Path, base: str, path: str) -> str | None:
    proc = git(repo, "show", f"{base}:{path}", check=False)
    return proc.stdout if proc.returncode == 0 else None


def commands(workflow: str) -> list[str]:
    """The workflow's shell lines, with backslash continuations joined."""
    out: list[str] = []
    pending = ""
    for raw in workflow.splitlines():
        line = raw.strip()
        if line.endswith("\\"):
            pending += line[:-1] + " "
            continue
        out.append(pending + line)
        pending = ""
    return out


def overlay_paths(workflow: str) -> tuple[list[str], list[str]]:
    """The strip-since.mjs path arguments and the pin-docs-counts.py pages."""
    strip = [c for c in commands(workflow) if c.startswith("node ci/scripts/strip-since.mjs")]
    if len(strip) != 1:
        raise SetupError(f"{WORKFLOW}: expected one strip-since.mjs command, found {len(strip)}")
    args = shlex.split(strip[0], posix=True)[2:]
    paths: list[str] = []
    skip = False
    for arg in args:
        if skip:
            skip = False
        elif arg == "--released-version":
            skip = True
        elif not arg.startswith("-"):
            paths.append(arg)
    if not paths:
        raise SetupError(f"{WORKFLOW}: strip-since.mjs is given no paths")
    pinned: list[str] = []
    for c in commands(workflow):
        if "ci/scripts/pin-docs-counts.py" in c:
            words = shlex.split(c, posix=True)
            pinned.append(words[-1])
    return paths, pinned


def markdown_files(repo: Path, path: str) -> list[str]:
    full = repo / path
    if full.is_file():
        return [path] if path.endswith(".md") else []
    if not full.is_dir():
        return []
    return sorted(str(p.relative_to(repo)) for p in full.rglob("*.md") if p.is_file())


def load_pinner():
    script = Path(__file__).resolve().parent / "pin-docs-counts.py"
    spec = importlib.util.spec_from_file_location("pin_docs_counts", script)
    if spec is None or spec.loader is None:
        raise SetupError(f"cannot load {script}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def pin_counts(repo: Path, base: str, rel: str, text: str) -> str:
    """Render a count-bearing page with the tag's facts, as docs-bundle.yml does."""
    facts = at_base(repo, base, "facts.json")
    if facts is None:
        raise SetupError(f"{base} has no facts.json to pin the count page with")
    pinner = load_pinner()
    with tempfile.TemporaryDirectory() as scratch:
        path = Path(scratch) / "facts.json"
        path.write_text(facts, encoding="utf-8")
        pinned, _summary = pinner.pin_named_page(path, Path(rel), text)
    return pinned


def coverage(lines: list[str]) -> tuple[list[bool], list[tuple[int, str]]]:
    """Which lines sit in a since-block (or are markers), and the bad markers.

    Mirrors strip-since.mjs: markers count only outside a fenced code block, and
    a fence line inside a block belongs to the block.
    """
    covered = [False] * len(lines)
    bad: list[tuple[int, str]] = []
    in_fence = False
    in_block = False
    for i, line in enumerate(lines):
        trimmed = line.strip()
        if trimmed.startswith("```"):
            in_fence = not in_fence
            covered[i] = in_block
            continue
        if not in_fence:
            if trimmed.startswith(SINCE_OPEN):
                version = trimmed[len(SINCE_OPEN):].strip()
                while version.endswith("-->"):
                    version = version[:-3].strip()
                if not VERSION.match(version):
                    bad.append((i, version))
                in_block = True
                covered[i] = True
                continue
            if trimmed == SINCE_CLOSE:
                in_block = False
                covered[i] = True
                continue
        covered[i] = in_block
    return covered, bad


def added_lines(old: str, new: str) -> list[int]:
    """0-based indexes of the lines of `new` that `old` does not have there."""
    a = old.splitlines()
    b = new.splitlines()
    out: list[int] = []
    matcher = difflib.SequenceMatcher(None, a, b, autojunk=False)
    for tag, _i1, _i2, j1, j2 in matcher.get_opcodes():
        if tag in ("insert", "replace"):
            out.extend(range(j1, j2))
    return out


def kind_token(title: str) -> str | None:
    m = re.search(r"`([a-z0-9_]+)`", title)
    return m.group(1) if m else None


def published_rules_lines(new: str, old: str) -> set[int]:
    """Lines of docs/rules.md that the rule-page bridge publishes from main."""
    released_kinds = {
        kind_token(line[4:])
        for line in old.splitlines()
        if line.startswith("### ")
    }
    released_kinds.discard(None)
    published: set[int] = set()
    h2 = ""
    h3_published = False
    for i, line in enumerate(new.splitlines()):
        if line.startswith("## "):
            h2 = line[3:].strip().lower()
            h3_published = False
            if h2.startswith("fix operations") or h2.startswith("nested"):
                published.add(i)
            continue
        if h2 == "contents" or h2.startswith("bundled rulesets"):
            continue
        if h2.startswith("fix operations") or h2.startswith("nested"):
            published.add(i)
            continue
        if line.startswith("### "):
            h3_published = kind_token(line[4:]) in released_kinds
        if h3_published:
            published.add(i)
    return published


def check(repo: Path, base: str) -> list[str]:
    workflow_path = repo / WORKFLOW
    if not workflow_path.is_file():
        raise SetupError(f"{WORKFLOW} not found under {repo}")
    if git(repo, "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}", check=False).returncode:
        raise SetupError(f"{base} is not a commit in {repo}")
    paths, pinned = overlay_paths(workflow_path.read_text(encoding="utf-8"))
    files: list[str] = []
    for path in paths:
        files.extend(f for f in markdown_files(repo, path) if f not in TRACKS_MAIN)
    files.append(RULES_MD)

    problems: list[str] = []
    for rel in sorted(set(files)):
        full = repo / rel
        if not full.is_file():
            continue
        new = full.read_text(encoding="utf-8")
        old = at_base(repo, base, rel) or ""
        if rel in pinned:
            new = pin_counts(repo, base, rel, new)
        lines = new.splitlines()
        covered, bad = coverage(lines)
        for i, version in bad:
            problems.append(f"{rel}:{i + 1}: since marker version {version!r} does not parse")
        scope = published_rules_lines(new, old) if rel == RULES_MD else None
        for i in added_lines(old, new):
            line = lines[i]
            if covered[i] or not line.strip() or ACTION_PIN.match(line):
                continue
            if scope is not None and i not in scope:
                continue
            problems.append(f"{rel}:{i + 1}: {line.strip()[:100]}")
    return problems


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--base", help="the release to compare with (default: the latest v* tag)")
    args = parser.parse_args(argv)
    repo = args.repo.resolve()
    try:
        base = args.base or base_tag(repo)
        problems = check(repo, base)
    except (SetupError, OSError, ValueError) as error:
        print(f"check-docs-overlay-since: {error}", file=sys.stderr)
        return 2
    if problems:
        print(
            f"check-docs-overlay-since: {len(problems)} line(s) added since {base} in docs "
            "that alint.org publishes from main sit outside a since-block:",
            file=sys.stderr,
        )
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        print(
            "Wrap each in <!-- alint:since=X --> ... <!-- /alint:since -->, X being the release "
            "that ships what it says: the next one for unreleased behaviour, or a released "
            f"version (such as {base.lstrip('v')}) for a correction of shipped behaviour.",
            file=sys.stderr,
        )
        return 1
    print(f"check-docs-overlay-since: every line added since {base} in the overlaid docs is in a since-block")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
