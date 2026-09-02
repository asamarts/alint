# Rule category assignments (Phase 0 curated table)

Status: proposed (2026-07-08). Companion to `docs/design/rule-categories.md` (the
many-to-many design) and ADR-0009 (the CLI). This is the curated kind-to-category
table the feature seeds from: Phase 1 lands each kind's PRIMARY only (behavior-neutral),
and Phase 3 flips the data to the full multi-membership recorded here.

## How to read this

- Each kind's **primary** is its current family (the H2 it sits under in `docs/rules.md`,
  which owns the canonical URL `/docs/rules/<primary>/<kind>/`). The primary is listed
  FIRST.
- **Secondaries** follow. Together they become the kind's `**Categories:**` line in
  `docs/rules.md`, primary first, by title. A kind with no secondary keeps its single
  family.
- Categories use titles (not slugs), matching the H2 headings.

## Editorial decisions applied

Three judgment calls and two minor defaults shaped the secondaries:

1. **Size/line limits carry Structure.** `file_max_size` / `file_min_size` /
   `file_max_lines` / `file_min_lines` are Content primary but also join Structure, so
   the numeric guardrails browse together. (Structure widens from "layout" to
   "layout + limits".)
2. **Three mis-filed kinds are cross-listed, not re-homed.** `commented_out_code`,
   `markdown_paths_resolve`, and `no_merge_conflict_markers` gain a secondary that puts
   them in the right browse, but their primary (and URL) is unchanged to avoid breaking
   live links. Their primaries are flagged below as candidates for a future re-home
   (which would need a site redirects story).
3. **Inclusive Security lens.** `file_content_forbidden`, `generated_file_fresh`, and
   `no_symlinks` all carry Security in addition to the clearly-security-first rules, for
   a comprehensive "show me security rules" view.
4. Minor defaults: `no_bom` gains Text hygiene (a clean-file concern); `filename_case`
   stays Naming-only (its portability angle is weaker than `no_case_conflicts`'s).

## The table

Format: `kind: Primary [, Secondary ...]`.

### Existence
```
file_exists: Existence
file_absent: Existence
dir_exists:  Existence
dir_absent:  Existence
```

### Content
```
file_content_matches:   Content
file_content_forbidden: Content, Security / Unicode sanity
file_header:            Content
file_starts_with:       Content
file_ends_with:         Content
file_hash:              Content, Security / Unicode sanity
file_max_size:          Content, Structure
file_min_size:          Content, Structure
file_min_lines:         Content, Structure
file_max_lines:         Content, Structure
file_footer:            Content
file_shebang:           Content, Unix metadata
file_is_text:           Content, Encoding
file_is_ascii:          Content, Encoding, Security / Unicode sanity
```

### Structured query
```
json_path_equals:   Structured query
yaml_path_equals:   Structured query
toml_path_equals:   Structured query
xml_path_equals:    Structured query
json_path_matches:  Structured query
yaml_path_matches:  Structured query
toml_path_matches:  Structured query
xml_path_matches:   Structured query
json_schema_passes: Structured query
```

### Naming
```
filename_case:  Naming
filename_regex: Naming
```

### Text hygiene
```
no_trailing_whitespace:      Text hygiene
final_newline:               Text hygiene
line_endings:                Text hygiene, Portable metadata
line_max_width:              Text hygiene
indent_style:                Text hygiene
max_consecutive_blank_lines: Text hygiene
```

### Security / Unicode sanity
```
no_merge_conflict_markers: Security / Unicode sanity, Text hygiene
no_bidi_controls:          Security / Unicode sanity, Encoding
no_zero_width_chars:       Security / Unicode sanity, Encoding
```

### Encoding
```
no_bom: Encoding, Text hygiene
```

### Structure
```
max_directory_depth:     Structure
max_files_per_directory: Structure
no_empty_files:          Structure
```

### Portable metadata
```
no_case_conflicts:        Portable metadata, Naming
no_illegal_windows_names: Portable metadata, Naming
```

### Unix metadata
```
no_symlinks:            Unix metadata, Portable metadata, Security / Unicode sanity
executable_bit:         Unix metadata
executable_has_shebang: Unix metadata, Content
shebang_has_executable: Unix metadata, Content
```

### Git hygiene
```
no_submodules:              Git hygiene
commented_out_code:         Git hygiene, Content
markdown_paths_resolve:     Git hygiene, Cross-file
git_no_denied_paths:        Git hygiene, Security / Unicode sanity
git_commit_message:         Git hygiene
git_commit_signed_off:      Git hygiene
git_commit_no_fixup:        Git hygiene
git_commit_subject_matches: Git hygiene
git_commit_author_allowlist: Git hygiene, Security / Unicode sanity
git_commit_gpg_signed:      Git hygiene, Security / Unicode sanity
git_blame_age:              Git hygiene
changeset_requires_path:    Git hygiene, Cross-file
pair_changed_together:      Git hygiene, Cross-file
```

### Cross-file
```
pair:                 Cross-file
pair_hash:            Cross-file, Security / Unicode sanity
registry_paths_resolve: Cross-file
cross_file:           Cross-file
file_graph:           Cross-file
ordered_block:        Cross-file, Text hygiene
for_each_match:       Cross-file
generated_file_fresh: Cross-file, Security / Unicode sanity
import_gate:          Cross-file, Security / Unicode sanity
command_idempotent:   Cross-file
for_each_dir:         Cross-file
for_each_file:        Cross-file
dir_contains:         Cross-file, Structure
dir_only_contains:    Cross-file, Structure
unique_by:            Cross-file
every_matching_has:   Cross-file
```

### Plugin (tier 1)
```
command: Plugin (tier 1)
```

## Exceptional 3-category kinds

The norm is at most two categories. Exactly two kinds carry three, and each earns it:

- `file_is_ascii` = Content + Encoding + Security / Unicode sanity. It is a content
  check, an encoding check, and a homoglyph/bidi defense at once.
- `no_symlinks` = Unix metadata + Portable metadata + Security / Unicode sanity. A Unix
  file-mode rule, a cross-platform portability rule, and a symlink-attack defense.

Nothing carries more than three; the `gen-categories` gate can assert this bound.

## Cross-cut lens totals (primary + secondary members)

| Category | Total kinds | (primary / gained as secondary) |
|---|---|---|
| Cross-file | 19 | 16 / +3 |
| Content | 17 | 14 / +3 |
| Security / Unicode sanity | 13 | 3 / +10 |
| Git hygiene | 13 | 13 / +0 |
| Structure | 9 | 3 / +6 |
| Text hygiene | 9 | 6 / +3 |
| Structured query | 9 | 9 / +0 |
| Encoding | 5 | 1 / +4 |
| Unix metadata | 5 | 4 / +1 |
| Existence | 4 | 4 / +0 |
| Naming | 4 | 2 / +2 |
| Portable metadata | 4 | 2 / +2 |
| Plugin (tier 1) | 1 | 1 / +0 |

The biggest discoverability gains are the Security lens (3 -> 13), Encoding (1 -> 5), and
Structure (3 -> 9). 32 of the 94 canonical kinds are multi-category; 62 stay single, so
the taxonomy is enriched without becoming a everything-tagged-everything soup.

## Mis-filed-primary flags (future cleanup, not this feature)

These kinds now appear in the right browse via a secondary, but their canonical
primary/URL is arguably wrong. Re-homing is deferred because it changes live URLs and
needs a redirects mechanism the site does not have:

- `commented_out_code`: primary Git hygiene, but it is a Content heuristic scan.
- `markdown_paths_resolve`: primary Git hygiene, but it is a link-integrity rule (the
  markdown analogue of `registry_paths_resolve`, which lives in Cross-file).
- `no_merge_conflict_markers`: primary Security / Unicode sanity, but leftover conflict
  markers are a correctness / text-hygiene concern.

## Related

- `docs/design/rule-categories.md` (the design), ADR-0009 (the CLI).
