#!/usr/bin/env python3
"""Validate generated rule-index links against a docs-bundle tree."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path, PurePosixPath
from urllib.parse import unquote, urlsplit


MARKDOWN_LINK = re.compile(r"\[[^\]]*\]\(([^\s)]+)")
RULE_PREFIX = "/docs/rules/"


def target_candidates(bundle: Path, href: str) -> tuple[Path, Path] | None:
    """Map an absolute rule-doc URL to its Markdown source candidates."""
    parsed = urlsplit(href)
    if parsed.scheme or parsed.netloc:
        return None
    path = unquote(parsed.path)
    if not path.startswith(RULE_PREFIX):
        return None

    relative = PurePosixPath(path.removeprefix("/docs/"))
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"unsafe rule-doc target {href!r}")

    target = bundle.joinpath(*relative.parts)
    if path.endswith("/"):
        return target.with_suffix(".md"), target / "index.md"
    return target.with_suffix(".md"), target / "index.md"


def index_pages(bundle: Path) -> list[Path]:
    rules = bundle / "rules"
    pages = [rules / "index.md"]
    pages.extend(sorted(rules.glob("*/index.md")))
    return [page for page in pages if page.is_file()]


def broken_links(bundle: Path) -> list[tuple[Path, str]]:
    broken: list[tuple[Path, str]] = []
    for page in index_pages(bundle):
        text = page.read_text(encoding="utf-8")
        for href in MARKDOWN_LINK.findall(text):
            try:
                candidates = target_candidates(bundle, href)
            except ValueError:
                broken.append((page, href))
                continue
            if candidates is not None and not any(path.is_file() for path in candidates):
                broken.append((page, href))
    return broken


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=Path)
    args = parser.parse_args()
    bundle = args.bundle.resolve()

    pages = index_pages(bundle)
    if not pages:
        print(f"docs-bundle rule-link check: no rule indexes under {bundle}", file=sys.stderr)
        return 2

    broken = broken_links(bundle)
    if broken:
        print(
            f"docs-bundle rule-link check: {len(broken)} unresolved rule link(s):",
            file=sys.stderr,
        )
        for page, href in broken:
            print(f"  {page.relative_to(bundle)}: {href}", file=sys.stderr)
        return 1

    print(f"docs-bundle rule-link check: {len(pages)} index page(s) OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
