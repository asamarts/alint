//! `markdown_fragments_resolve` — require local Markdown link fragments to
//! name generated heading anchors in their target documents.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, RelatedLocation, Result, Rule, RuleSpec, Scope,
    SourceRange, Violation, missing_file_violation, too_large_file_violation,
    unreadable_file_violation,
};
use serde::Deserialize;

use crate::markdown_links_resolve::{
    MarkdownLinksRootMapSpec, RelativeMode, RootMap, build_root_map, has_uri_scheme,
    normalize_link_path, percent_decode,
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// How to handle relative destinations. Under `forbid`, another-file links
    /// are left to `markdown_links_resolve`; same-document fragments still run.
    #[serde(default)]
    #[schemars(extend("default" = "resolve"))]
    relative: RelativeMode,
    /// Optional rendered-site root URL to repository source mapping.
    #[serde(default)]
    root: Option<MarkdownLinksRootMapSpec>,
}

crate::options_schema_for!(Options);

#[derive(Debug)]
pub struct MarkdownFragmentsResolveRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
    relative: RelativeMode,
    root_map: Option<RootMap>,
}

#[derive(Debug)]
enum AnchorState {
    Ready(HashSet<String>),
    Failed,
}

impl Rule for MarkdownFragmentsResolveRule {
    alint_core::rule_common_impl!();

    fn requires_full_index(&self) -> bool {
        // A changed target heading can invalidate links in unchanged sources.
        true
    }

    fn supports_expect_matches(&self) -> bool {
        true
    }

    fn scope_matches_any(&self, index: &alint_core::FileIndex) -> bool {
        self.scope
            .candidate_files(index)
            .any(|entry| self.scope.matches(&entry.path, index))
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let mut anchors: HashMap<PathBuf, AnchorState> = HashMap::new();

        for entry in self.scope.candidate_files(ctx.index) {
            if !self.scope.matches(&entry.path, ctx.index) {
                continue;
            }
            let bytes = match crate::io::read_capped(&ctx.root.join(&entry.path)) {
                Ok(bytes) => bytes,
                Err(crate::io::ReadCapError::TooLarge(size)) => {
                    violations.push(too_large_file_violation(&entry.path, size));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    violations.push(missing_file_violation(&entry.path));
                    continue;
                }
                Err(crate::io::ReadCapError::Io(error)) => {
                    violations.push(unreadable_file_violation(&entry.path, &error));
                    continue;
                }
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                violations.push(
                    Violation::new("Markdown file is not valid UTF-8")
                        .with_path(Arc::clone(&entry.path))
                        .as_analysis_failure(AnalysisFailureKind::ResourceParse),
                );
                continue;
            };
            for link in crate::markdown::links(text) {
                let Some((target, fragment)) = self.resolve_target(ctx, &entry.path, &link.target)
                else {
                    continue;
                };
                if !anchors.contains_key(&target) {
                    let state = load_anchors(ctx, &target, &mut violations);
                    anchors.insert(target.clone(), state);
                }
                let Some(AnchorState::Ready(target_anchors)) = anchors.get(&target) else {
                    continue;
                };
                if target_anchors.contains(fragment.as_ref()) {
                    continue;
                }

                let message = self.message.clone().unwrap_or_else(|| {
                    format!(
                        "Markdown fragment `#{}` does not name a heading in `{}`",
                        fragment,
                        crate::slash(&target)
                    )
                });
                let mut violation = Violation::new(message)
                    .with_path(Arc::clone(&entry.path))
                    .with_baseline_key(format!("{}\0{}", crate::slash(&target), fragment));
                if let Some(range) = SourceRange::from_byte_range(text, link.span) {
                    violation = violation.with_range(range);
                }
                violation = violation.with_related_location(RelatedLocation::new(
                    target,
                    "Markdown fragment target document",
                ));
                violations.push(violation);
            }
        }
        Ok(violations)
    }
}

impl MarkdownFragmentsResolveRule {
    fn resolve_target<'a>(
        &self,
        ctx: &Context<'_>,
        source: &Path,
        raw_target: &'a str,
    ) -> Option<(PathBuf, std::borrow::Cow<'a, str>)> {
        let target = raw_target.trim();
        if target.is_empty() || target.starts_with("//") || has_uri_scheme(target) {
            return None;
        }
        let (before_fragment, fragment) = target.split_once('#')?;
        if fragment.is_empty() {
            return None;
        }
        let fragment = percent_decode(fragment);
        let path_part = before_fragment.split('?').next().unwrap_or("");
        if path_part.is_empty() {
            return markdown_target(ctx, source, source).map(|path| (path, fragment));
        }

        let root_absolute = path_part.starts_with('/') || path_part.starts_with('\\');
        let decoded = percent_decode(path_part).replace('\\', "/");
        let candidate = if root_absolute {
            let map = self.root_map.as_ref()?;
            let suffix = decoded.strip_prefix(&map.url_prefix)?;
            let relative = if suffix.is_empty() {
                PathBuf::new()
            } else {
                normalize_link_path(Path::new(suffix))?
            };
            map.dir.join(relative)
        } else {
            if self.relative == RelativeMode::Forbid {
                return None;
            }
            let base = source.parent().unwrap_or_else(|| Path::new(""));
            normalize_link_path(&base.join(decoded.trim_start_matches('/')))?
        };
        markdown_target(ctx, source, &candidate).map(|path| (path, fragment))
    }
}

fn markdown_target(ctx: &Context<'_>, source: &Path, candidate: &Path) -> Option<PathBuf> {
    if candidate == source && ctx.index.contains_file(candidate) {
        return Some(candidate.to_path_buf());
    }
    if ctx.index.contains_file(candidate) && has_markdown_extension(candidate) {
        return Some(candidate.to_path_buf());
    }
    if candidate.extension().is_none() {
        let mut with_md = candidate.to_path_buf();
        with_md.set_extension("md");
        if ctx.index.contains_file(&with_md) {
            return Some(with_md);
        }
    }
    for name in ["README.md", "index.md"] {
        let nested = candidate.join(name);
        if ctx.index.contains_file(&nested) {
            return Some(nested);
        }
    }
    None
}

fn has_markdown_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md")
                || extension.eq_ignore_ascii_case("markdown")
                || extension.eq_ignore_ascii_case("mdx")
        })
}

fn load_anchors(ctx: &Context<'_>, path: &Path, violations: &mut Vec<Violation>) -> AnchorState {
    let bytes = match crate::io::read_capped(&ctx.root.join(path)) {
        Ok(bytes) => bytes,
        Err(crate::io::ReadCapError::TooLarge(size)) => {
            violations.push(too_large_file_violation(path, size));
            return AnchorState::Failed;
        }
        Err(crate::io::ReadCapError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            violations.push(missing_file_violation(path));
            return AnchorState::Failed;
        }
        Err(crate::io::ReadCapError::Io(error)) => {
            violations.push(unreadable_file_violation(path, &error));
            return AnchorState::Failed;
        }
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        violations.push(
            Violation::new("Markdown fragment target is not valid UTF-8")
                .with_path(path.to_path_buf())
                .as_analysis_failure(AnalysisFailureKind::ResourceParse),
        );
        return AnchorState::Failed;
    };
    AnchorState::Ready(
        crate::markdown::generated_anchors(&crate::markdown::headings(text))
            .into_iter()
            .collect(),
    )
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed("scope_filter is not supported by this kind"),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[alint_core::SelectorMeta {
        name: std::borrow::Cow::Borrowed("root.dir"),
        selector_type: alint_core::SelectorType::LiteralPath,
        rerootable: true,
    }]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::OtherFiles,
    formats: std::borrow::Cow::Borrowed(&[]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: true,
    trace: alint_core::TraceMeta::SELECTION,
    nested_rules: false,
};

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    if spec.paths.is_none() {
        return Err(Error::rule_config(
            &spec.id,
            "markdown_fragments_resolve requires a `paths` field",
        ));
    }
    alint_core::reject_scope_filter_on_cross_file(spec, "markdown_fragments_resolve")?;
    let options: Options = spec
        .deserialize_options()
        .map_err(|error| Error::rule_config(&spec.id, format!("invalid options: {error}")))?;
    let root_map = build_root_map(&spec.id, options.root)?;
    Ok(Box::new(MarkdownFragmentsResolveRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
        relative: options.relative,
        root_map,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn resolves_same_file_relative_and_generated_duplicate_anchors() {
        let rule = super::build(&spec_yaml(
            "id: fragments\nkind: markdown_fragments_resolve\npaths: 'docs/*.md'\nlevel: error\n",
        ))
        .unwrap();
        let source = concat!(
            "# Source\n",
            "[self](#source)\n",
            "[valid](target.md#topic)\n",
            "[duplicate](target.md#topic-1)\n",
            "[bad](target.md#missing)\n",
            "[external](https://example.com/x#missing)\n",
        );
        let target = "# Target\n## Topic\n## Topic\n";
        let (tmp, index) = tempdir_with_files(&[
            ("docs/source.md", source.as_bytes()),
            ("docs/target.md", target.as_bytes()),
        ]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(5));
        assert_eq!(
            violations[0].path.as_deref(),
            Some(Path::new("docs/source.md"))
        );
        assert_eq!(
            violations[0].related_locations[0].path,
            Path::new("docs/target.md")
        );
    }

    #[test]
    fn resolves_percent_encoded_and_rendered_route_fragments() {
        let rule = super::build(&spec_yaml(
            "id: fragments\nkind: markdown_fragments_resolve\npaths: README.md\nroot:\n  url_prefix: /docs/\n  dir: site\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[
            ("README.md", b"[guide](/docs/guide/#caf%C3%A9)\n"),
            ("site/guide/index.md", "# Café\n".as_bytes()),
        ]);
        assert!(rule.evaluate(&ctx(tmp.path(), &index)).unwrap().is_empty());
    }

    #[test]
    fn invalid_target_utf8_fails_once_for_repeated_links() {
        let rule = super::build(&spec_yaml(
            "id: fragments\nkind: markdown_fragments_resolve\npaths: README.md\nlevel: error\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[
            ("README.md", b"[one](bad.md#a) [two](bad.md#b)\n"),
            ("bad.md", b"# bad\xff\n"),
        ]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(alint_core::AnalysisFailureKind::ResourceParse)
        );
    }
}
