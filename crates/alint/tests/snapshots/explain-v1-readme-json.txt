{
  "capabilities": [
    {
      "capability": {
        "kind": "git",
        "operations": [
          "diff"
        ]
      },
      "condition": {
        "kind": "common_field_set",
        "field": "scope_filter.changed_since"
      }
    }
  ],
  "categories": [
    "content"
  ],
  "conditional": true,
  "docs": "https://alint.org/docs/rules/content/file_content_matches/",
  "expect_matches": false,
  "fix": null,
  "fix_ops": [
    "file_append"
  ],
  "fixable": false,
  "id": "readme-title",
  "kind": "rule",
  "level": "warning",
  "message": "every README starts with a title\n(see the style guide)\n",
  "options": {
    "pattern": "^# "
  },
  "paths": {
    "exclude": [
      "packages/legacy/README.md",
      "packages/vendor/**"
    ],
    "include": [
      "packages/*/README.md"
    ]
  },
  "policy_url": "https://example.com/style",
  "rule_kind": "file_content_matches",
  "schema_version": 1,
  "scope_filter": {
    "changed_since": null,
    "has_ancestor": [
      "package.json"
    ],
    "manifest_scopes": [
      {
        "paths": [
          "packages/a",
          "packages/b"
        ],
        "predicate": "include_manifest_paths",
        "source": "package.json"
      }
    ]
  },
  "selectors": [],
  "summary": "File contents must contain at least one match for a regex.",
  "when": "facts.has_rust"
}
