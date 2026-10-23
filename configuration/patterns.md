---
title: Regex patterns and anchors
description: What each rule kind matches a regex against, what ^ and $ mean there, and the flags that change it.
sidebar:
  order: 7
---

Rule options such as `pattern:`, `select:`, `matches:` and `regex:` take [Rust regex syntax](https://docs.rs/regex/latest/regex/#syntax). The syntax is the same everywhere, but what a pattern is matched against depends on the rule kind, and that decides what `^` and `$` mean.

## What a pattern is matched against

| The pattern sees | Kinds | `^` and `$` match at |
|---|---|---|
| one line at a time | `for_each_match`, `import_gate`, `ordered_block`, `git_blame_age` | the start and end of each line |
| the whole file | `file_content_forbidden`, `file_content_matches`, the `file_content_matches` fact, `extract: { regex: ... }`, `file_graph` edges | the start and end of the file |
| the first or last N lines, newlines included | `file_header`, `file_footer` | the start and end of that block |
| the whole commit message | `git_commit_message` | the start and end of the message |
| one value | `filename_regex`, `git_commit_subject_matches`, the structured `*_path_matches` kinds, a fact's `extract:`ed values | the start and end of the value |

## Whole-file patterns

In the whole-file kinds, `^` and `$` anchor to the start and end of the file, not of each line. Two consequences surprise anyone used to `grep`:

- `^TODO` matches only when the file starts with `TODO`. To match a line that starts with it, turn on multi-line mode: `(?m)^TODO`.
- `$` does not match before a final newline, so `TODO$` misses every file that ends in a newline, which is almost every text file. Use `(?m)TODO$` for the end of a line, or `\s*$` to allow trailing whitespace and the final newline at the end of the file.

In multi-line mode `$` matches before `\n` only, so on files with CRLF line endings also turn on CRLF mode: `(?Rm)TODO$`.

To anchor to the whole file on purpose, `\A` and `\z` say so explicitly:

| You mean | Write |
|---|---|
| a line that starts with `X` | `(?m)^X` |
| a line that ends with `X` | `(?m)X$`, or `(?Rm)X$` with CRLF line endings |
| the file starts with `X` | `\AX` |
| the file ends with `X`, final newline allowed | `X\s*\z` |
| the file is exactly `X` | `\AX\s*\z` |

For example, `\A\d+\.\d+\.\d+\s*\z` asserts that a `VERSION` file holds one version and nothing else. `file_content_forbidden` reports the first match in each file, so `(?m)^TODO` flags a file once, at the first line that starts with `TODO`.

## Warnings for mis-anchored patterns

When a config loads, alint warns about two shapes that almost never do what the author meant:

- a `$` that only text without a final newline can reach, such as `[ \t]+$` or `^\d+\.\d+\.\d+$`;
- a `file_content_forbidden` pattern that starts with `^` and does not end with `$` or `\z`, such as `^TODO`.

```text
alint: warning: rule `no-todo`: `^` matches only at the start of the file, not at the start of each line; use `(?m)^` to anchor to lines, or `\A` if you mean the start of the file
```

The warnings appear on `check`, `fix`, `baseline` and `validate-config` (whose JSON output lists them under `warnings`), and as diagnostics on the config file in an editor. They never change findings or the exit code, and `--quiet` silences them. A pattern that turns on `(?m)` anywhere, or that uses `\A` and `\z` instead of `^` and `$`, gets no warning.

## Other flags

Inline flags can be set at the start of a pattern or a group: `(?i)` matches case-insensitively, `(?s)` lets `.` match a newline, and `(?x)` allows whitespace and comments in the pattern. They combine, as in `(?im)^todo`, and a group can scope them: `(?m:^todo)`. The `regex` crate's [flags reference](https://docs.rs/regex/latest/regex/#grouping-and-flags) lists them all.
