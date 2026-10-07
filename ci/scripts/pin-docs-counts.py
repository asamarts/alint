#!/usr/bin/env python3
"""Pin the count-bearing concepts page to a release's facts.json contract."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


def release_counts(facts_path: Path) -> tuple[int, int, int]:
    facts = json.loads(facts_path.read_text(encoding="utf-8"))
    counts = facts.get("counts")
    accepted = facts.get("rule_kinds")
    aliases = facts.get("rule_aliases")
    categories = facts.get("rule_categories")
    if not isinstance(counts, dict) or not isinstance(accepted, list):
        raise ValueError("facts.json must contain counts and a rule_kinds list")
    if not isinstance(aliases, dict) or not isinstance(categories, dict):
        raise ValueError("facts.json must contain rule_aliases and rule_categories maps")

    alias_count = len(aliases)
    format_version = facts.get("format_version")
    if not isinstance(format_version, int):
        raise ValueError("facts.json format_version must be an integer")
    if format_version >= 3:
        canonical = counts.get("rule_kinds")
        declared_aliases = counts.get("rule_aliases")
        if declared_aliases != alias_count:
            raise ValueError(
                f"facts.json declares {declared_aliases} aliases but maps {alias_count}"
            )
    else:
        legacy_total = counts.get("rule_kinds")
        if not isinstance(legacy_total, int):
            raise ValueError("legacy facts.json counts.rule_kinds must be an integer")
        canonical = legacy_total - alias_count

    if not isinstance(canonical, int) or canonical < 0:
        raise ValueError("facts.json canonical rule count must be a non-negative integer")
    if len(accepted) != canonical + alias_count:
        raise ValueError(
            "facts.json accepted-name list does not equal canonical kinds plus aliases"
        )
    if len(categories) != canonical:
        raise ValueError("facts.json rule_categories must contain every canonical kind")
    multi_category = sum(
        1
        for memberships in categories.values()
        if isinstance(memberships, list) and len(memberships) > 1
    )
    return canonical, alias_count, multi_category


def replace_once(text: str, pattern: str, replacement: str, label: str) -> str:
    updated, count = re.subn(pattern, replacement, text, count=1)
    if count != 1:
        raise ValueError(f"count page no longer has exactly one {label} field")
    return updated


def pin_page(text: str, canonical: int, aliases: int, multi_category: int) -> str:
    remaining = canonical - 4  # Four named examples precede "and N more".
    replacements = [
        (
            r'(description: "alint ships )\d+( rule kinds plus )\d+( aliases,)',
            rf"\g<1>{canonical}\g<2>{aliases}\g<3>",
            "frontmatter count",
        ),
        (
            r'(alint ships \*\*)\d+( rule kinds\*\* plus )\d+( aliases,)',
            rf"\g<1>{canonical}\g<2>{aliases}\g<3>",
            "intro count",
        ),
        (
            r'(Each of )\d+( rule kinds belongs)',
            rf"\g<1>{canonical}\g<2>",
            "accessible-diagram canonical count",
        ),
        (
            r'\d+( aliases provide alternate names)',
            rf"{aliases}\g<1>",
            "accessible-diagram alias count",
        ),
        (
            r'\d+( kinds \+ )\d+( aliases &#183;)',
            rf"{canonical}\g<1>{aliases}\g<2>",
            "visible-diagram count",
        ),
        (
            r'(categories; )\d+( kinds carry more than one,)',
            rf"\g<1>{multi_category}\g<2>",
            "visible-diagram multi-category count",
        ),
        (
            r'(json_schema_passes`, and )\d+( more\.)',
            rf"\g<1>{remaining}\g<2>",
            "remaining-kind count",
        ),
        (
            r'\d+( \*\*aliases\*\* provide alternate names)',
            rf"{aliases}\g<1>",
            "body alias count",
        ),
        (
            r'(separately from the )\d+( kinds\.)',
            rf"\g<1>{canonical}\g<2>",
            "body canonical count",
        ),
        (
            r'(The )\d+( kinds are partitioned)',
            rf"\g<1>{canonical}\g<2>",
            "family canonical count",
        ),
        (
            r'(and \*\*)\d+( kinds carry extra tags\*\*)',
            rf"\g<1>{multi_category}\g<2>",
            "multi-category prose count",
        ),
    ]
    for pattern, replacement, label in replacements:
        text = replace_once(text, pattern, replacement, label)
    return text


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("facts", type=Path)
    parser.add_argument("page", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    try:
        canonical, aliases, multi_category = release_counts(args.facts)
        original = args.page.read_text(encoding="utf-8")
        pinned = pin_page(original, canonical, aliases, multi_category)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"pin-docs-counts: {error}", file=sys.stderr)
        return 2

    if args.check:
        if pinned != original:
            print(
                f"pin-docs-counts: {args.page} does not match {args.facts}",
                file=sys.stderr,
            )
            return 1
    else:
        args.page.write_text(pinned, encoding="utf-8")

    print(
        "pin-docs-counts: "
        f"{canonical} canonical kinds, {aliases} aliases, "
        f"{multi_category} multi-category kinds OK"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
