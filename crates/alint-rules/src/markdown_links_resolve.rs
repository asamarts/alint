//! Resolve Markdown links against the repository tree.
//!
//! Unlike `markdown_paths_resolve`, which checks path-shaped inline-code
//! claims, this rule understands Markdown inline links, images, reference
//! definitions, and explicit reference uses. It deliberately ignores fenced
//! and indented code, inline code spans, front matter, and HTML comments.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RelativeMode {
    /// Resolve relative destinations against the Markdown source file.
    #[default]
    Resolve,
    /// Reject relative destinations (useful for trailing-slash documentation
    /// sites, where filesystem-relative links resolve against a different URL).
    Forbid,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct RootMapSpec {
    /// Root-absolute URL prefix to validate, including leading and trailing
    /// slashes (for example `/docs/`). Other root-absolute URLs are ignored.
    url_prefix: String,
    /// Repository directory corresponding to `url_prefix` (for example
    /// `docs/site`). Extensionless routes also try `<route>.md` and
    /// `<route>/index.md`.
    dir: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// How to handle relative Markdown destinations: `resolve` (default) or
    /// `forbid`.
    #[serde(default)]
    #[schemars(extend("default" = "resolve"))]
    relative: RelativeMode,
    /// Optional mapping from root-absolute rendered URLs to repository source
    /// files. This validates only URLs beneath the declared prefix.
    #[serde(default)]
    root: Option<RootMapSpec>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
struct RootMap {
    url_prefix: String,
    dir: PathBuf,
}

#[derive(Debug)]
pub struct MarkdownLinksResolveRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    relative: RelativeMode,
    root_map: Option<RootMap>,
}

impl Rule for MarkdownLinksResolveRule {
    alint_core::rule_common_impl!();

    // Link targets may be anywhere in the repository, including outside the
    // changed set. Treat this as a cross-file rule: a non-empty changed run
    // checks the complete documentation graph, and LSP refreshes it on save.
    fn requires_full_index(&self) -> bool {
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        for entry in ctx.index.files() {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let bytes = match crate::io::read_capped(&ctx.root.join(&entry.path)) {
                Ok(bytes) => bytes,
                Err(crate::io::ReadCapError::TooLarge(size)) => {
                    violations.push(
                        Violation::new(format!(
                            "Markdown file is too large to analyze ({})",
                            crate::io::over_cap(size)
                        ))
                        .with_path(Arc::clone(&entry.path))
                        .with_baseline_key("read"),
                    );
                    continue;
                }
                Err(crate::io::ReadCapError::Io(err)) => {
                    violations.push(
                        Violation::new(format!("could not read Markdown file: {err}"))
                            .with_path(Arc::clone(&entry.path))
                            .with_baseline_key("read"),
                    );
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue;
            };
            let scanned = scan_markdown(text);
            for link in scanned.links {
                if let Some(problem) = self.validate_target(ctx, &entry.path, &link.target) {
                    let message = self.message.clone().unwrap_or(problem);
                    violations.push(
                        Violation::new(message)
                            .with_path(Arc::clone(&entry.path))
                            .with_location(link.line, link.column)
                            .with_baseline_key(link.target),
                    );
                }
            }
            for reference in scanned.undefined_references {
                let message = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "Markdown reference label `[{}]` is not defined",
                        reference.label
                    )
                });
                violations.push(
                    Violation::new(message)
                        .with_path(Arc::clone(&entry.path))
                        .with_location(reference.line, reference.column)
                        .with_baseline_key(format!("reference: {}", reference.label)),
                );
            }
        }
        Ok(violations)
    }
}

impl MarkdownLinksResolveRule {
    fn validate_target(
        &self,
        ctx: &Context<'_>,
        source: &Path,
        raw_target: &str,
    ) -> Option<String> {
        let target = raw_target.trim();
        if target.is_empty()
            || target.starts_with('#')
            || target.starts_with("//")
            || has_uri_scheme(target)
        {
            return None;
        }

        let path_part = target.split(['#', '?']).next().unwrap_or("");
        if path_part.is_empty() {
            return None;
        }
        let decoded = percent_decode(path_part);

        if decoded.starts_with('/') {
            let map = self.root_map.as_ref()?;
            let suffix = decoded.strip_prefix(&map.url_prefix)?;
            let mapped = if suffix.is_empty() {
                map.dir.clone()
            } else {
                let Some(relative) = alint_core::normalize_confined(Path::new(suffix)) else {
                    return Some(format!(
                        "root-absolute Markdown link `{raw_target}` escapes its configured source directory"
                    ));
                };
                map.dir.join(relative)
            };
            if rendered_route_exists(ctx, &mapped) {
                return None;
            }
            return Some(format!(
                "root-absolute Markdown link `{raw_target}` maps to `{}` but no source file or directory exists",
                crate::slash(&mapped)
            ));
        }

        if self.relative == RelativeMode::Forbid {
            return Some(format!(
                "relative Markdown link `{raw_target}` is forbidden; use a root-absolute or external URL"
            ));
        }

        let base = source.parent().unwrap_or_else(|| Path::new(""));
        let Some(resolved) = alint_core::normalize_confined(&base.join(decoded.as_ref())) else {
            return Some(format!(
                "relative Markdown link `{raw_target}` escapes the repository root"
            ));
        };
        if ctx.index.contains_path(&resolved) {
            None
        } else {
            Some(format!(
                "Markdown link `{raw_target}` resolves to `{}` but no file or directory exists",
                crate::slash(&resolved)
            ))
        }
    }
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_links_resolve requires a `paths` field",
        ));
    }
    alint_core::reject_scope_filter_on_cross_file(spec, "markdown_links_resolve")?;
    let opts: Options = spec
        .deserialize_options()
        .map_err(|e| Error::rule_config(&spec.id, format!("invalid options: {e}")))?;
    let root_map = match opts.root {
        Some(root) => {
            if !root.url_prefix.starts_with('/')
                || !root.url_prefix.ends_with('/')
                || root.url_prefix.starts_with("//")
            {
                return Err(Error::rule_config(
                    &spec.id,
                    "`root.url_prefix` must start and end with `/` (for example `/docs/`)",
                ));
            }
            let Some(dir) = alint_core::normalize_confined(Path::new(&root.dir)) else {
                return Err(Error::rule_config(
                    &spec.id,
                    "`root.dir` must be a non-empty repository-relative path",
                ));
            };
            Some(RootMap {
                url_prefix: root.url_prefix,
                dir,
            })
        }
        None => None,
    };
    Ok(Box::new(MarkdownLinksResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        relative: opts.relative,
        root_map,
    }))
}

fn rendered_route_exists(ctx: &Context<'_>, mapped: &Path) -> bool {
    if ctx.index.contains_path(mapped) {
        return true;
    }
    if mapped.extension().is_none() {
        let mut markdown = mapped.to_path_buf();
        markdown.set_extension("md");
        if ctx.index.contains_file(&markdown) {
            return true;
        }
        if ctx.index.contains_file(&mapped.join("index.md")) {
            return true;
        }
    }
    false
}

fn has_uri_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.chars().enumerate().all(|(idx, ch)| {
            ch.is_ascii_alphabetic()
                || (idx > 0 && (ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.')))
        })
}

fn percent_decode(input: &str) -> std::borrow::Cow<'_, str> {
    if !input.as_bytes().contains(&b'%') {
        return std::borrow::Cow::Borrowed(input);
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_or(std::borrow::Cow::Borrowed(input), std::borrow::Cow::Owned)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LinkTarget {
    target: String,
    line: usize,
    column: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct UndefinedReference {
    label: String,
    line: usize,
    column: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ScanResult {
    links: Vec<LinkTarget>,
    undefined_references: Vec<UndefinedReference>,
}

#[derive(Debug)]
struct ReferenceUse {
    label: String,
    line: usize,
    column: usize,
}

/// Extract live Markdown links while retaining byte offsets for diagnostics.
fn scan_markdown(text: &str) -> ScanResult {
    let visible = mask_non_link_regions(text);
    let line_starts = line_starts(&visible);
    let mut result = ScanResult::default();
    let mut definitions = HashSet::new();

    for (line_idx, line) in visible.split_inclusive('\n').enumerate() {
        if let Some((label, target, target_col)) = parse_reference_definition(line) {
            definitions.insert(normalize_label(&label));
            result.links.push(LinkTarget {
                target,
                line: line_idx + 1,
                column: target_col,
            });
        }
    }

    scan_inline_links(&visible, &line_starts, &mut result.links);
    result.links.sort_by_key(|link| (link.line, link.column));
    let references = scan_reference_uses(&visible, &line_starts);
    result.undefined_references = references
        .into_iter()
        .filter(|reference| !definitions.contains(&normalize_label(&reference.label)))
        .map(|reference| UndefinedReference {
            label: reference.label,
            line: reference.line,
            column: reference.column,
        })
        .collect();
    result
}

fn mask_non_link_regions(text: &str) -> String {
    let mut out = text.as_bytes().to_vec();
    let mut offset = 0;
    let mut in_fence: Option<(u8, usize)> = None;
    let mut in_frontmatter = false;
    let mut frontmatter_possible = true;
    let mut in_comment = false;

    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = content.trim();

        if frontmatter_possible {
            frontmatter_possible = false;
            if trimmed == "---" {
                in_frontmatter = true;
                blank(&mut out, offset, offset + line.len());
                offset += line.len();
                continue;
            }
        } else if in_frontmatter {
            blank(&mut out, offset, offset + line.len());
            if trimmed == "---" {
                in_frontmatter = false;
            }
            offset += line.len();
            continue;
        }

        let fence_text = content.trim_start_matches(' ');
        let indent = content.len() - fence_text.len();
        if indent <= 3
            && let Some((marker, count)) = fence_run(fence_text)
        {
            match in_fence {
                None => in_fence = Some((marker, count)),
                Some((open, required))
                    if marker == open
                        && count >= required
                        && fence_text[count..].trim().is_empty() =>
                {
                    in_fence = None;
                }
                Some(_) => {}
            }
            blank(&mut out, offset, offset + line.len());
            offset += line.len();
            continue;
        }
        if in_fence.is_some() || content.starts_with("    ") || content.starts_with('\t') {
            blank(&mut out, offset, offset + line.len());
            offset += line.len();
            continue;
        }

        let bytes = content.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if in_comment {
                if let Some(end) = find_bytes(bytes, i, b"-->") {
                    blank(&mut out, offset + i, offset + end + 3);
                    i = end + 3;
                    in_comment = false;
                } else {
                    blank(&mut out, offset + i, offset + bytes.len());
                    break;
                }
            } else if bytes[i..].starts_with(b"<!--") {
                if let Some(end) = find_bytes(bytes, i + 4, b"-->") {
                    blank(&mut out, offset + i, offset + end + 3);
                    i = end + 3;
                } else {
                    blank(&mut out, offset + i, offset + bytes.len());
                    in_comment = true;
                    break;
                }
            } else if bytes[i] == b'`' {
                let run = byte_run(bytes, i, b'`');
                if let Some(close) = find_exact_run(bytes, i + run, b'`', run) {
                    blank(&mut out, offset + i, offset + close + run);
                    i = close + run;
                } else {
                    i += run;
                }
            } else {
                i += 1;
            }
        }
        offset += line.len();
    }
    // CommonMark code spans may cross a line break. The per-line pass above
    // handles the overwhelmingly common case while it has comment state; this
    // second pass closes any still-visible matching backtick run globally.
    mask_multiline_code_spans(&mut out);
    String::from_utf8(out).expect("masking valid UTF-8 with ASCII spaces preserves UTF-8")
}

fn mask_multiline_code_spans(bytes: &mut [u8]) {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = byte_run(bytes, i, b'`');
        if let Some(close) = find_exact_run(bytes, i + run, b'`', run) {
            blank(bytes, i, close + run);
            i = close + run;
        } else {
            i += run;
        }
    }
}

fn scan_inline_links(text: &str, starts: &[usize], out: &mut Vec<LinkTarget>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] != b']' || bytes[i + 1] != b'(' || is_escaped(bytes, i) {
            i += 1;
            continue;
        }
        if find_link_opener(bytes, i).is_none() {
            i += 2;
            continue;
        }
        let mut cursor = i + 2;
        let mut depth = 1usize;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = (cursor + 2).min(bytes.len());
                continue;
            }
            match bytes[cursor] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            break;
        }
        if let Some((target, relative_start)) = parse_destination(&text[i + 2..cursor]) {
            let absolute = i + 2 + relative_start;
            let (line, column) = offset_location(starts, absolute);
            out.push(LinkTarget {
                target,
                line,
                column,
            });
        }
        i = cursor + 1;
    }
}

fn scan_reference_uses(text: &str, starts: &[usize]) -> Vec<ReferenceUse> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] != b']' || bytes[i + 1] != b'[' || is_escaped(bytes, i) {
            i += 1;
            continue;
        }
        let Some(open) = find_link_opener(bytes, i) else {
            i += 2;
            continue;
        };
        let Some(close_rel) = text[i + 2..].find(']') else {
            break;
        };
        let close = i + 2 + close_rel;
        if text[i + 2..close].contains('\n') {
            i += 2;
            continue;
        }
        let explicit = &text[i + 2..close];
        let label = if explicit.is_empty() {
            &text[open + 1..i]
        } else {
            explicit
        };
        if !label.trim().is_empty() {
            let (line, column) = offset_location(starts, i + 1);
            out.push(ReferenceUse {
                label: unescape_markdown(label.trim()),
                line,
                column,
            });
        }
        i = close + 1;
    }
    out
}

fn parse_reference_definition(line: &str) -> Option<(String, String, usize)> {
    let without_newline = line.strip_suffix('\n').unwrap_or(line);
    let trimmed = without_newline.trim_start_matches(' ');
    let indent = without_newline.len() - trimmed.len();
    if indent > 3 || !trimmed.starts_with('[') {
        return None;
    }
    let close = find_unescaped(trimmed.as_bytes(), 1, b']')?;
    if trimmed.as_bytes().get(close + 1) != Some(&b':') {
        return None;
    }
    let label = unescape_markdown(trimmed[1..close].trim());
    // GitHub-style footnote definitions use the same `[label]: body` shape,
    // but their body is prose rather than a link destination.
    if label.is_empty() || label.starts_with('^') {
        return None;
    }
    let rest = &trimmed[close + 2..];
    let (target, start) = parse_destination(rest)?;
    Some((label, target, indent + close + 2 + start + 1))
}

/// Parse one `CommonMark` destination and ignore any following link title.
fn parse_destination(input: &str) -> Option<(String, usize)> {
    let start = input.len() - input.trim_start().len();
    let rest = &input[start..];
    if rest.is_empty() {
        return None;
    }
    if let Some(angle) = rest.strip_prefix('<') {
        let close = find_unescaped(angle.as_bytes(), 0, b'>')?;
        let target = unescape_markdown(&angle[..close]);
        return (!target.is_empty()).then_some((target, start + 1));
    }
    let bytes = rest.as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        if bytes[end] == b'\\' && end + 1 < bytes.len() {
            end += 2;
            continue;
        }
        if bytes[end].is_ascii_whitespace() {
            break;
        }
        end += 1;
    }
    let target = unescape_markdown(&rest[..end]);
    (!target.is_empty()).then_some((target, start))
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn unescape_markdown(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() {
            out.push(bytes[i + 1]);
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("removing ASCII escape bytes preserves UTF-8")
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(text.match_indices('\n').map(|(idx, _)| idx + 1));
    starts
}

fn offset_location(starts: &[usize], offset: usize) -> (usize, usize) {
    let line_idx = starts.partition_point(|&start| start <= offset) - 1;
    (line_idx + 1, offset - starts[line_idx] + 1)
}

fn find_link_opener(bytes: &[u8], close: usize) -> Option<usize> {
    let line_start = bytes[..close]
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(0, |idx| idx + 1);
    (line_start..close)
        .rev()
        .find(|&idx| bytes[idx] == b'[' && !is_escaped(bytes, idx))
}

fn is_escaped(bytes: &[u8], idx: usize) -> bool {
    let mut slashes = 0;
    let mut cursor = idx;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        slashes += 1;
        cursor -= 1;
    }
    slashes % 2 == 1
}

fn fence_run(text: &str) -> Option<(u8, usize)> {
    let marker = *text.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let count = byte_run(text.as_bytes(), 0, marker);
    (count >= 3).then_some((marker, count))
}

fn byte_run(bytes: &[u8], start: usize, byte: u8) -> usize {
    bytes[start..].iter().take_while(|&&b| b == byte).count()
}

fn find_exact_run(bytes: &[u8], start: usize, byte: u8, len: usize) -> Option<usize> {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] != byte {
            i += 1;
            continue;
        }
        let run = byte_run(bytes, i, byte);
        if run == len {
            return Some(i);
        }
        i += run;
    }
    None
}

fn find_unescaped(bytes: &[u8], start: usize, needle: u8) -> Option<usize> {
    (start..bytes.len()).find(|&idx| bytes[idx] == needle && !is_escaped(bytes, idx))
}

fn find_bytes(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    bytes[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|idx| start + idx)
}

fn blank(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ctx, index_with_dirs, spec_yaml, tempdir_with_files};

    fn spec(extra: &str) -> RuleSpec {
        spec_yaml(&format!(
            "id: docs-links\nkind: markdown_links_resolve\npaths: \"**/*.md\"\nlevel: error\n{extra}"
        ))
    }

    #[test]
    fn scanner_finds_inline_image_definition_and_undefined_reference() {
        let result = scan_markdown(
            "[inline](../a.md \"title\") ![image](img/a.png)\n\
             [guide]: <guide.md> 'title'\n\
             [^note]: prose that is not a link destination\n\
             [defined][guide] [missing][nowhere]\n",
        );
        assert_eq!(
            result
                .links
                .iter()
                .map(|link| link.target.as_str())
                .collect::<Vec<_>>(),
            ["../a.md", "img/a.png", "guide.md"]
        );
        assert_eq!(
            result.undefined_references,
            [UndefinedReference {
                label: "nowhere".into(),
                line: 4,
                column: 27,
            }]
        );
    }

    #[test]
    fn scanner_skips_code_comments_frontmatter_and_indented_blocks() {
        let text = "---\ntitle: '[front](bad.md)'\n---\n\
                    `[inline](bad.md)` and [`code text`](live.md)\n\
                    <!-- [comment](bad.md) -->\n\
                    ```md\n[fenced](bad.md)\n```\n    [indented](bad.md)\n";
        let text = format!("{text}`multi-line\n[also hidden](bad.md)`\n");
        let result = scan_markdown(&text);
        assert_eq!(result.links.len(), 1, "{result:#?}");
        assert_eq!(result.links[0].target, "live.md");
        assert_eq!(result.links[0].line, 4);
    }

    #[test]
    fn relative_links_resolve_from_source_directory_and_decode_urls() {
        let (tmp, idx) = tempdir_with_files(&[
            (
                "docs/guide.md",
                b"[readme](../README.md) [space](space%20name.md)\n",
            ),
            ("README.md", b"# readme\n"),
            ("docs/space name.md", b"# space\n"),
        ]);
        let rule = build(&spec("")).unwrap();
        assert!(rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn missing_and_escaping_relative_links_fire_per_link() {
        let (tmp, idx) = tempdir_with_files(&[(
            "docs/guide.md",
            b"[missing](missing.md) and [escape](../../outside.md)\n",
        )]);
        let rule = build(&spec("")).unwrap();
        let violations = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].line, Some(1));
        assert_eq!(violations[0].column, Some(11));
        assert!(violations[0].message.contains("docs/missing.md"));
        assert!(violations[1].message.contains("escapes"));
    }

    #[test]
    fn forbid_mode_allows_fragments_root_urls_and_external_schemes() {
        let (tmp, idx) = tempdir_with_files(&[(
            "docs/guide.md",
            b"[bad](other.md) [self](#part) [root](/docs/) [web](https://example.com) [mail](mailto:x@example.com)\n",
        )]);
        let rule = build(&spec("relative: forbid\n")).unwrap();
        let violations = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(violations.len(), 1, "{violations:#?}");
        assert!(violations[0].message.contains("forbidden"));
    }

    #[test]
    fn root_map_resolves_exact_markdown_and_index_routes() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = index_with_dirs(&[
            ("docs/site", true),
            ("docs/site/about.md", false),
            ("docs/site/api", true),
            ("docs/site/api/index.md", false),
            ("docs/site/source.md", false),
            ("docs/secret.md", false),
        ]);
        std::fs::create_dir_all(tmp.path().join("docs/site/api")).unwrap();
        std::fs::write(
            tmp.path().join("docs/site/source.md"),
            "[about](/docs/about/) [api](/docs/api/) [bad](/docs/missing/) [escape](/docs/%2E%2E/secret.md)\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("docs/site/about.md"), "# about\n").unwrap();
        std::fs::write(tmp.path().join("docs/site/api/index.md"), "# api\n").unwrap();
        std::fs::write(tmp.path().join("docs/secret.md"), "# secret\n").unwrap();
        let rule = build(&spec(
            "relative: forbid\nroot:\n  url_prefix: /docs/\n  dir: docs/site\n",
        ))
        .unwrap();
        let violations = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(violations.len(), 2, "{violations:#?}");
        assert!(violations[0].message.contains("docs/site/missing"));
        assert!(violations[1].message.contains("escapes"));
    }

    #[test]
    fn build_rejects_bad_root_map_and_scope_filter() {
        assert!(build(&spec("root: { url_prefix: docs, dir: docs/site }\n")).is_err());
        assert!(build(&spec("root: { url_prefix: /docs/, dir: ../outside }\n")).is_err());
        assert!(build(&spec("scope_filter: { has_ancestor: package.json }\n")).is_err());
    }
}
