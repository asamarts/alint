---
title: 'pair_changed_together'
description: 'If the <since>...HEAD diff changes any path matching if_changed:, at least one path matching then_changed: must change in the same range,.'
sidebar:
  order: 13
---

If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range — the **co-change** gate. Corpus signals: rust's `rustdoc-json-types` `FORMAT_VERSION` must bump when the format struct changes; "`version.txt` and the lockfile change together" release guards. Both globs and `since:` (the base ref) are required. **Directional** — the trigger is `if_changed`, the obligation is `then_changed`; a `then_changed`-only change never fires it, so add a second rule with the globs swapped for a bidirectional pact. The `changeset_requires_path` sibling, built on the same merge-base diff as `alint check --changed`. Silent no-op outside a git repo or when `if_changed` didn't change; a `since:` that fails to resolve hard-fails with a shallow-clone hint.

```yaml
- id: format-version-bumped
  kind: pair_changed_together
  if_changed: "src/rustdoc-json-types/lib.rs"     # if the format struct changes…
  then_changed: "src/rustdoc-json-types/FORMAT_VERSION"  # …the version file must too
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

---

