id:         readme-title
kind:       file_content_matches
categories: content
summary:    File contents must contain at least one match for a regex.
capabilities: git(diff) [when scope_filter.changed_since is set]
selector fields: none
fix_ops:     file_append
docs:       https://alint.org/docs/rules/content/file_content_matches/
level:      warning
paths:      packages/*/README.md  (excluding packages/legacy/README.md, packages/vendor/**)
scope_filter:
  has_ancestor:  package.json
  include_manifest_paths: package.json -> packages/a, packages/b
options:    pattern: ^# 
message:    every README starts with a title
            (see the style guide)
policy_url: https://example.com/style
when:       facts.has_rust
