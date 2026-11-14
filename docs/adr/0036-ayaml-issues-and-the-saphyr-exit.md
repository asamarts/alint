---
status: accepted
date: 2026-11-14
decision-makers: asamarts
---

# 0036. File ayaml gaps as ayaml issues, and remove saphyr in v0.24

## Status

Accepted (2026-11-14). Amends [ADR-0031](0031-v0221-owner-decisions.md) decision 3 for ayaml: gaps
found in ayaml are filed as GitHub issues on asamarts/ayaml. adotenv gaps and alint-side work are
filed as before. [ADR-0035](0035-keep-the-early-ayaml-cutover.md) stands: ayaml stays on `main` and
is qualified before v0.23.0.

## Context

- **Where ayaml gaps went.** ADR-0031 decision 3 routed them to ayaml's maintainers, at their
  request. The maintainers checked each gap against ayaml's own tasks and added generic public
  tasks, whose text names no consumer.
- **The maintainers agreed to a change.** On 2026-11-14 they agreed that gaps be filed directly as
  GitHub issues on asamarts/ayaml. The issues keep ayaml's convention for public text, which names
  no consumer and no comparison crate. The six gaps found so far are filed:
  - asamarts/ayaml#123: plain-scalar spans and `raw` include trailing whitespace;
  - asamarts/ayaml#124: the default count limits refuse valid documents of 0.3 to 7 MB, far below
    the 32 MiB input limit;
  - asamarts/ayaml#125: the compatibility mode has no lazy event stream;
  - asamarts/ayaml#126: document that event readers report an alias to an undefined anchor without
    an error;
  - asamarts/ayaml#127: the total bytes allocated per read are about twice a libyaml-based
    reader's (ayaml tasks AY-070 and AY-057);
  - asamarts/ayaml#128: a spanned tree with path lookup (ayaml task AY-049).
- **alint runs two YAML parsers.** #510 moved alint's decoding to ayaml, through
  `alint_core::yaml`. saphyr and saphyr-parser 0.3.3 stay for four uses:
  - source spans of extracted values (`alint-core/src/extract.rs`);
  - structured YAML fixes over `saphyr::MarkedYaml` (`alint-core/src/structured_fix/`);
  - `yaml_depth::mapping_subtree_is_literal`, which rejects anchors, aliases, tags and merge keys
    below a path before typed decoding;
  - `pinned_references`' event scan (`alint-rules/src/pinned_references.rs`).

  Two parsers can disagree on whether a document is valid. alint can then decode a file with one
  and fail to locate or fix a value in it with the other.
- **What moving them needs.** The four uses can read ayaml's events instead. A plain scalar's span
  and `raw` text include trailing whitespace (asamarts/ayaml#123), so until that is fixed alint
  would trim them itself. A spanned tree with path lookup (asamarts/ayaml#128) would replace the
  lookups that `structured_fix` builds over saphyr's tree. #482's differential harness and #483's
  re-derived depth guard qualify ayaml first (ADR-0035), so the spans move onto a qualified parser.

## Decision

1. **ayaml gaps are filed as issues on asamarts/ayaml.** Each issue states the gap in ayaml's
   terms, in generic text that names no consumer and no comparison crate. An issue may cite
   ayaml's own task ids. This supersedes ADR-0031 decision 3's routing through the maintainers'
   task tracker, with the maintainers' agreement of 2026-11-14. adotenv gaps go to adotenv issues
   and alint-side work to alint issues, as before.
2. **saphyr leaves in v0.24.** After #482 and #483, alint's four saphyr uses move to ayaml's
   events, and `saphyr` and `saphyr-parser` leave the dependency graph.
   - The move depends on asamarts/ayaml#123, or on alint trimming plain-scalar spans itself.
   - It benefits from asamarts/ayaml#128.
   - v0.23 keeps saphyr.

## Consequences

- **v0.23's scope does not change.** The v0.23 plan and the roadmap list the saphyr exit under
  v0.24.
- **Two parsers until v0.24.** ayaml decodes and saphyr locates and fixes, and they can still
  disagree on validity. #482's harness compares ayaml with `yaml_serde`, not with saphyr.
- **The span and fix code is rewritten once,** against a parser that #482 and #483 have qualified.
- **ayaml gaps are public when found.** A gap alint finds appears in ayaml's tracker without
  naming alint.
- **Records.** The v0.23 plan, the roadmap and the [`yaml-parser.md`](../design/yaml-parser.md)
  re-evaluation log record the change.

## Considered Options

- **Where ayaml gaps go:**
  - issues on asamarts/ayaml, in generic text (chosen, with the maintainers' agreement);
  - through the maintainers' task tracker (ADR-0031 decision 3): their text stays under their
    control, with a step between finding a gap and its public record.
- **When saphyr leaves:**
  - v0.24, after #482 and #483 (chosen);
  - extend v0.23 to move the four uses: about 4 to 6 days more, and v0.23.0 would depend on an
    ayaml release;
  - keep saphyr indefinitely: no work, but two parsers that can disagree on validity stay in the
    graph.

## More Information

- [ADR-0031](0031-v0221-owner-decisions.md) decision 3 and
  [ADR-0035](0035-keep-the-early-ayaml-cutover.md).
- [`yaml-parser.md`](../design/yaml-parser.md): the re-evaluation log.
- [`v0.23/README.md`](../design/v0.23/README.md): the plan, phases 5 and 9.
- Issues #481, #482, #483 and #484; pull request #510; asamarts/ayaml#123 to asamarts/ayaml#128.
