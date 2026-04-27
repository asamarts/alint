---
title: 'compliance/apache-2@v1'
description: Bundled alint ruleset at alint://bundled/compliance/apache-2@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/compliance/apache-2@v1
```

## Rules

### `apache-2-license-text-present`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0>

> Apache-2.0 compliance: LICENSE must contain the Apache License, Version 2.0 text. Pull the canonical copy from https://www.apache.org/licenses/LICENSE-2.0.txt.

### `apache-2-notice-file-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0#redistribution>

> Apache-2.0 §4(d): distributions that include a NOTICE from upstream must propagate it. Even if your direct dependencies don't ship one, having a project-level NOTICE for your own attributions is the canonical Apache pattern.

### `apache-2-source-has-license-header`

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0#apply>

> Apache-2.0: source files should carry the canonical Apache header (Copyright + "Licensed under the Apache License, Version 2.0"). The full boilerplate is at https://www.apache.org/licenses/LICENSE-2.0#apply.

