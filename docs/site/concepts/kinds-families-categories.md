---
title: Kinds, families, and categories
description: "alint ships 94 rule kinds (105 with aliases), grouped one way by family (one home each) and another way by category (cross-cutting tags you can filter on)."
sidebar:
  order: 4
---

Every rule names a `kind`: the built-in check it runs. alint ships **94 distinct kinds** (105 counting aliases), and it organizes them two ways at once. Each kind has one home **family**, and each kind is tagged into one or more **categories**. Same thirteen names, two different relationships.

<svg class="alint-kinds" viewBox="0 0 460 440" role="img" aria-labelledby="kfc-t kfc-d" xmlns="http://www.w3.org/2000/svg">
<title id="kfc-t">Kinds, families, and categories</title>
<desc id="kfc-d">Each rule kind belongs to exactly one family (its colored home group) but can be tagged into several categories. file_exists is in the Existence family and the existence category. no_bidi_controls is in the Security family, tagged security and encoding. dir_contains is in the Cross-file family, tagged cross-file and structure. filename_case is in the Naming family and the naming category. 105 kinds, 13 families, 13 categories.</desc>
<style>
  .alint-kinds { --tx:#1e1b4b; --mut:#64748b; --card:#ffffff; --bd:#c7cfe0; --ac:#4f46e5; width:100%; max-width:480px; height:auto; font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  :root[data-theme="dark"] .alint-kinds { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; }
  @media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) .alint-kinds { --tx:#e6e8ef; --mut:#93a0b8; --card:#2a2f3e; --bd:#3b4254; --ac:#8b93f8; } }
  .alint-kinds .mono { font:600 13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
  .alint-kinds .ui { font:600 12px system-ui, -apple-system, sans-serif; }
  .alint-kinds .tx { fill:var(--tx); } .alint-kinds .mut { fill:var(--mut); } .alint-kinds .ac { fill:var(--ac); }
  .alint-kinds .card { fill:var(--card); stroke:var(--bd); stroke-width:1.3; }
  .alint-kinds .chip { fill:none; stroke:var(--bd); stroke-width:1.1; }
  .alint-kinds .extra { animation:kpulse 2.6s ease-in-out infinite; }
  @keyframes kpulse { 0%,100% { opacity:.55; } 50% { opacity:1; } }
  @media (prefers-reduced-motion:reduce){ .alint-kinds .extra { animation:none; opacity:1; } }
</style>
<text class="ui ac" x="18" y="15">one family, many categories</text>
<text class="ui mut" x="18" y="33">105 kinds &#183; 13 families &#183; 13 categories</text>
<rect class="card" x="20" y="46" width="420" height="58" rx="8"/><rect x="20" y="46" width="6" height="58" rx="2" fill="#3b82f6"/>
<text class="mono tx" x="38" y="72" font-size="14">file_exists</text>
<rect x="330" y="56" width="92" height="22" rx="11" fill="#3b82f6"/><text class="ui" x="376" y="71" text-anchor="middle" fill="#fff">Existence</text>
<rect class="chip" x="38" y="82" width="76" height="18" rx="9"/><text class="mono mut" x="48" y="95" font-size="10">existence</text>
<rect class="card" x="20" y="116" width="420" height="58" rx="8"/><rect x="20" y="116" width="6" height="58" rx="2" fill="#ef4444"/>
<text class="mono tx" x="38" y="142" font-size="14">no_bidi_controls</text>
<rect x="336" y="126" width="86" height="22" rx="11" fill="#ef4444"/><text class="ui" x="379" y="141" text-anchor="middle" fill="#fff">Security</text>
<rect class="chip" x="38" y="152" width="70" height="18" rx="9"/><text class="mono mut" x="48" y="165" font-size="10">security</text>
<g class="extra"><rect class="chip" x="114" y="152" width="72" height="18" rx="9"/><text class="mono mut" x="124" y="165" font-size="10">encoding</text></g>
<rect class="card" x="20" y="186" width="420" height="58" rx="8"/><rect x="20" y="186" width="6" height="58" rx="2" fill="#7c3aed"/>
<text class="mono tx" x="38" y="212" font-size="14">dir_contains</text>
<rect x="330" y="196" width="92" height="22" rx="11" fill="#7c3aed"/><text class="ui" x="376" y="211" text-anchor="middle" fill="#fff">Cross-file</text>
<rect class="chip" x="38" y="222" width="78" height="18" rx="9"/><text class="mono mut" x="48" y="235" font-size="10">cross-file</text>
<g class="extra"><rect class="chip" x="126" y="222" width="74" height="18" rx="9"/><text class="mono mut" x="136" y="235" font-size="10">structure</text></g>
<rect class="card" x="20" y="256" width="420" height="58" rx="8"/><rect x="20" y="256" width="6" height="58" rx="2" fill="#06b6d4"/>
<text class="mono tx" x="38" y="282" font-size="14">filename_case</text>
<rect x="346" y="266" width="76" height="22" rx="11" fill="#06b6d4"/><text class="ui" x="384" y="281" text-anchor="middle" fill="#fff">Naming</text>
<rect class="chip" x="38" y="292" width="64" height="18" rx="9"/><text class="mono mut" x="48" y="305" font-size="10">naming</text>
<line x1="20" y1="336" x2="440" y2="336" stroke="var(--bd)" stroke-width="1" opacity=".5"/>
<rect x="20" y="352" width="14" height="14" rx="3" fill="#7c3aed"/><text class="ui tx" x="42" y="363">colored band = the one family a kind belongs to</text>
<rect class="chip" x="20" y="380" width="14" height="14" rx="7"/><text class="ui tx" x="42" y="391">chips = its categories; 32 kinds carry more than one,</text>
<text class="ui mut" x="42" y="410">so categories cross-cut the families</text>
</svg>

## Kinds are the checks

A `kind` is the built-in implementation a rule invokes: `file_exists`, `no_bidi_controls`, `filename_case`, `json_schema_passes`, and 90 more. Every rule declares exactly one, and the `kind` is what decides which extra fields the rule accepts (a `file_header` takes a `pattern`, a `file_max_size` takes a byte limit). A handful of kinds have **aliases**, second names for the same implementation, which is why the catalog counts 105 entries for 94 distinct checks.

## Families are the home group

The 94 kinds are partitioned into **13 families** by mechanism: Existence, Content, Naming, Structure, Cross-file, Security / Unicode sanity, Text hygiene, Encoding, Portable metadata, Unix metadata, Git hygiene, Structured query, and Plugin. Every kind belongs to **exactly one** family, so the families are a clean table of contents for the catalog. `alint rules list` prints the kinds grouped this way.

## Categories are cross-cutting tags

The **13 categories** carry the same thirteen names, but the relationship is many-to-many: a kind can be tagged with several. A kind's home family is always its primary category, and **32 kinds carry extra tags** on top. `no_bidi_controls` lives in the Security family but is also tagged `encoding`; `dir_contains` lives in Cross-file but is also tagged `structure`. That is the whole distinction: **family answers "where does this kind live?" (one answer), category answers "what concerns does it touch?" (often several).** Categories are what you filter on when you want every check that bears on, say, security, regardless of which family implements it.

## In practice

Browse the catalog by family, or filter it by category:

```
alint rules list                       # every kind alint ships, grouped by family
alint rules list --category security   # only the kinds tagged "security"
alint list --category security         # only YOUR configured rules that are "security"
```

An unknown category slug fails fast with the list of the thirteen valid ones, so a typo never silently returns nothing.

## Going deeper

- [Rules](/docs/rules/) is the full reference for every kind, grouped by family, with each kind's fields and examples.
- [The config model](/docs/concepts/the-config-model/) shows how a `kind` sits inside the rule record.
