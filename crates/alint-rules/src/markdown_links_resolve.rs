//! Resolve Markdown links against the repository tree.
//!
//! Unlike `markdown_paths_resolve`, which checks path-shaped inline-code
//! claims, this rule understands Markdown inline links, images, reference
//! definitions, and explicit reference uses. Parsing is delegated to a
//! `CommonMark` parser so code blocks, code spans, front matter, HTML comments,
//! escaping, nested labels, and multiline constructs follow rendered-Markdown
//! semantics rather than a second ad-hoc grammar.

use std::cell::RefCell;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use alint_core::{Context, Error, Level, Result, Rule, RuleSpec, Scope, Violation};
use pulldown_cmark::{BrokenLink, Event, LinkType, Options as MarkdownOptions, Parser, Tag};
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
struct MarkdownLinksRootMapSpec {
    /// Root-absolute URL prefix to validate, including leading and trailing
    /// slashes (for example `/docs/`). Other root-absolute URLs are ignored.
    url_prefix: String,
    /// Repository directory corresponding to `url_prefix` (for example
    /// `docs/site`, or `.` for the repository root). Extensionless routes also
    /// try `<route>.md` and `<route>/index.md`.
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
    root: Option<MarkdownLinksRootMapSpec>,
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

    // This cross-file rule deliberately has no `path_scope`: in changed mode,
    // deleting an otherwise-unmodified target must still re-check every source
    // document. Its source side is nevertheless an enumerable file scope, so
    // the common opt-in empty-scope assertion remains meaningful.
    fn supports_expect_matches(&self) -> bool {
        true
    }

    fn scope_matches_any(&self, index: &alint_core::FileIndex) -> bool {
        index
            .files()
            .any(|entry| self.scope.matches(&entry.path, index))
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
        // URL paths use `/` on every host. Treat literal or percent-encoded
        // backslashes as separators too, so traversal and lookup semantics do
        // not change between Unix and Windows runners (and match browsers that
        // normalize backslashes in special-scheme URLs).
        let decoded = decoded.replace('\\', "/");

        if decoded.starts_with('/') {
            let map = self.root_map.as_ref()?;
            let suffix = decoded.strip_prefix(&map.url_prefix)?;
            let mapped = if suffix.is_empty() {
                map.dir.clone()
            } else {
                let Some(relative) = normalize_link_path(Path::new(suffix)) else {
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
        let Some(resolved) = normalize_link_path(&base.join(&decoded)) else {
            return Some(format!(
                "relative Markdown link `{raw_target}` escapes the repository root"
            ));
        };
        if resolved.as_os_str().is_empty() || ctx.index.contains_path(&resolved) {
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
            let Some(dir) = (!root.dir.is_empty())
                .then(|| normalize_link_path(Path::new(&root.dir)))
                .flatten()
            else {
                return Err(Error::rule_config(
                    &spec.id,
                    "`root.dir` must be a repository-relative directory (use `.` for the repository root)",
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
    if mapped.as_os_str().is_empty() || ctx.index.contains_path(mapped) {
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

/// Normalize a repository-relative link path while allowing the repository
/// root itself as a valid directory target. The shared `normalize_confined`
/// helper intentionally rejects an empty result because most rule references
/// need a concrete file; Markdown links are the exception (`..` may validly
/// resolve from a top-level directory to the repository root).
fn normalize_link_path(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(component) => out.push(component),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
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

/// Extract live Markdown links while retaining source offsets for diagnostics.
///
/// `pulldown-cmark` owns the grammar decisions here. In particular, malformed
/// link-looking prose is not treated as a link, only the first duplicate
/// reference definition is active, and links nested in any valid code block or
/// metadata block never reach this event stream.
fn scan_markdown(text: &str) -> ScanResult {
    // A UTF-8 BOM is transparent document metadata. `pulldown-cmark` expects
    // metadata delimiters at byte zero, so omit it from parsing while adding
    // its byte width back to every reported source span.
    let markdown = text.strip_prefix('\u{feff}').unwrap_or(text);
    let source_offset = text.len() - markdown.len();
    let undefined_references = RefCell::new(Vec::new());
    let callback = |broken: BrokenLink<'_>| {
        if matches!(
            broken.link_type,
            LinkType::Reference
                | LinkType::ReferenceUnknown
                | LinkType::Collapsed
                | LinkType::CollapsedUnknown
        ) {
            let span = absolute_span(broken.span, source_offset);
            let offset = reference_label_offset(text, &span);
            let (line, column) = offset_location(text, offset);
            undefined_references.borrow_mut().push(UndefinedReference {
                label: broken.reference.into_string(),
                line,
                column,
            });
        }
        None
    };

    let mut options = MarkdownOptions::empty();
    options.insert(
        MarkdownOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | MarkdownOptions::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
            | MarkdownOptions::ENABLE_FOOTNOTES,
    );
    let mut parser =
        Parser::new_with_broken_link_callback(markdown, options, Some(callback)).into_offset_iter();
    let mut result = ScanResult::default();

    for (event, span) in parser.by_ref() {
        let Event::Start(
            Tag::Link {
                link_type: LinkType::Inline,
                dest_url,
                ..
            }
            | Tag::Image {
                link_type: LinkType::Inline,
                dest_url,
                ..
            },
        ) = event
        else {
            continue;
        };
        let span = absolute_span(span, source_offset);
        let offset = inline_destination_offset(text, &span);
        let (line, column) = offset_location(text, offset);
        result.links.push(LinkTarget {
            target: dest_url.into_string(),
            line,
            column,
        });
    }

    // Reference definitions are populated as the parser advances. Read them
    // only after exhausting the event stream, including definitions that are
    // never referenced by body text.
    for (_, definition) in parser.reference_definitions().iter() {
        let span = absolute_span(definition.span.clone(), source_offset);
        let offset = definition_destination_offset(text, &span);
        let (line, column) = offset_location(text, offset);
        result.links.push(LinkTarget {
            target: definition.dest.to_string(),
            line,
            column,
        });
    }

    drop(parser);
    result.undefined_references = undefined_references.into_inner();
    result.links.sort_by_key(|link| (link.line, link.column));
    result
}

fn absolute_span(span: Range<usize>, offset: usize) -> Range<usize> {
    span.start + offset..span.end + offset
}

/// Find the destination start within a parser-confirmed inline link. This is
/// diagnostic-only: resolution always uses the parser's decoded `dest_url`.
fn inline_destination_offset(text: &str, span: &Range<usize>) -> usize {
    let source = &text[span.clone()];
    let bytes = source.as_bytes();
    for close in (0..bytes.len().saturating_sub(1)).rev() {
        if bytes[close] == b']' && bytes.get(close + 1) == Some(&b'(') {
            let rest = &source[close + 2..];
            let whitespace = rest.len() - rest.trim_start().len();
            let angle = usize::from(rest[whitespace..].starts_with('<'));
            return span.start + close + 2 + whitespace + angle;
        }
    }
    span.start
}

/// Find the destination start within a parser-confirmed reference definition.
/// Definitions may put the destination on the following line.
fn definition_destination_offset(text: &str, span: &Range<usize>) -> usize {
    let source = &text[span.clone()];
    let bytes = source.as_bytes();
    let mut close = 1;
    while close < bytes.len() {
        if bytes[close] == b']' && !is_escaped(bytes, close) {
            break;
        }
        close += 1;
    }
    if bytes.get(close + 1) != Some(&b':') {
        return span.start;
    }
    let rest = &source[close + 2..];
    let whitespace = rest.len() - rest.trim_start().len();
    let angle = usize::from(rest[whitespace..].starts_with('<'));
    span.start + close + 2 + whitespace + angle
}

fn reference_label_offset(text: &str, span: &Range<usize>) -> usize {
    text[span.clone()]
        .rfind("][")
        .map_or(span.start, |offset| span.start + offset + 1)
}

/// Convert a UTF-8 byte offset to one-based Unicode-scalar line and column.
/// This keeps diagnostics aligned for links following non-ASCII prose.
fn offset_location(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |idx| idx + 1);
    let column = text[line_start..offset.min(text.len())].chars().count() + 1;
    (line, column)
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
             \n\
             [guide]: <guide.md> 'title'\n\
             [^note]: prose that is not a link destination\n\
             \n\
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
                line: 6,
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
    fn scanner_uses_commonmark_structure_in_nested_and_multiline_content() {
        let result = scan_markdown(concat!(
            "> ```md\n",
            "> [quoted fence](hidden.md)\n",
            "> ```\n",
            "\n",
            "- item\n",
            "\n",
            "        [indented](hidden.md)\n",
            "\n",
            "[multiline\nlabel](guide.md)\n",
            "[malformed](not-a-link.md\"without whitespace\")\n",
        ));
        assert_eq!(
            result
                .links
                .iter()
                .map(|link| link.target.as_str())
                .collect::<Vec<_>>(),
            ["guide.md"]
        );
    }

    #[test]
    fn scanner_handles_metadata_variants_and_yaml_end_marker() {
        for text in [
            "---\ntitle: '[hidden](bad.md)'\n...\n[live](good.md)\n",
            "\u{feff}---\ntitle: '[hidden](bad.md)'\n---\n[live](good.md)\n",
            "+++\ntitle = '[hidden](bad.md)'\n+++\n[live](good.md)\n",
        ] {
            let result = scan_markdown(text);
            assert_eq!(result.links.len(), 1, "{result:#?}");
            assert_eq!(result.links[0].target, "good.md");
            assert_eq!(result.links[0].line, 4);
        }
    }

    #[test]
    fn scanner_uses_only_the_active_reference_definition() {
        let result = scan_markdown(
            "[use][duplicate]\n\
             \n\
             [duplicate]: good.md\n\
             [duplicate]: missing.md\n",
        );
        assert_eq!(
            result
                .links
                .iter()
                .map(|link| link.target.as_str())
                .collect::<Vec<_>>(),
            ["good.md"]
        );
        assert_eq!(result.undefined_references, []);
    }

    #[test]
    fn scanner_reports_unicode_columns_and_decodes_entities() {
        let result = scan_markdown("café [target](space&#x20;name.md)\n");
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "space name.md");
        assert_eq!((result.links[0].line, result.links[0].column), (1, 15));
    }

    #[test]
    fn relative_links_resolve_from_source_directory_and_decode_urls() {
        let (tmp, idx) = tempdir_with_files(&[
            (
                "docs/guide.md",
                b"[readme](../README.md) [space](space%20name.md) [root](..)\n",
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
            b"[missing](missing.md) [escape](../../outside.md) [encoded](..%5C..%5Coutside.md)\n",
        )]);
        let rule = build(&spec("")).unwrap();
        let violations = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(violations.len(), 3);
        assert_eq!(violations[0].line, Some(1));
        assert_eq!(violations[0].column, Some(11));
        assert!(violations[0].message.contains("docs/missing.md"));
        assert!(violations[1].message.contains("escapes"));
        assert!(violations[2].message.contains("escapes"));
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
            "[about](/docs/about/) [api](/docs/api/) [bad](/docs/missing/) \
             [root](/docs/./) \
             [escape](/docs/%2E%2E/secret.md) \
             [encoded](/docs/%2E%2E%5Csecret.md)\n",
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
        assert_eq!(violations.len(), 3, "{violations:#?}");
        assert!(violations[0].message.contains("docs/site/missing"));
        assert!(violations[1].message.contains("escapes"));
        assert!(violations[2].message.contains("escapes"));
    }

    #[test]
    fn build_rejects_bad_root_map_and_scope_filter() {
        assert!(build(&spec("root: { url_prefix: docs, dir: docs/site }\n")).is_err());
        assert!(build(&spec("root: { url_prefix: /docs/, dir: ../outside }\n")).is_err());
        assert!(build(&spec("scope_filter: { has_ancestor: package.json }\n")).is_err());
    }

    #[test]
    fn root_map_can_target_repository_root() {
        let (tmp, idx) = tempdir_with_files(&[
            ("source.md", b"[root](/) [readme](/README.md)\n"),
            ("README.md", b"# readme\n"),
        ]);
        let rule = build(&spec("root: { url_prefix: /, dir: . }\n")).unwrap();
        assert!(rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().is_empty());
    }

    #[test]
    fn whole_graph_rule_supports_expect_matches_without_changed_scope_skipping() {
        let rule = build(&spec("")).unwrap();
        let index = index_with_dirs(&[("README.txt", false)]);
        assert!(rule.requires_full_index());
        assert!(rule.path_scope().is_none());
        assert!(rule.supports_expect_matches());
        assert!(!rule.scope_matches_any(&index));
    }
}
