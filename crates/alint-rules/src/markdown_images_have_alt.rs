//! `markdown_images_have_alt` — require non-empty rendered alt text on every
//! `CommonMark` image.

use std::path::Path;
use std::sync::Arc;

use alint_core::{
    AnalysisFailureKind, Context, Error, Level, PerFileRule, Result, Rule, RuleSpec, Scope,
    SourceRange, Violation, eval_per_file,
};

#[derive(Debug)]
pub struct MarkdownImagesHaveAltRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    scope: Scope,
}

impl Rule for MarkdownImagesHaveAltRule {
    alint_core::rule_common_impl!();

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        eval_per_file(self, ctx)
    }

    fn as_per_file(&self) -> Option<&dyn PerFileRule> {
        Some(self)
    }
}

impl PerFileRule for MarkdownImagesHaveAltRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(vec![
                Violation::new("Markdown file is not valid UTF-8")
                    .with_path(Arc::<Path>::from(path))
                    .as_analysis_failure(AnalysisFailureKind::ResourceParse),
            ]);
        };
        let mut violations = Vec::new();
        let mut seen_dests: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for image in crate::markdown::images(text) {
            if !image.alt.trim().is_empty() {
                continue;
            }
            let occ = seen_dests.entry(image.dest.clone()).or_insert(0);
            *occ += 1;
            let message = self
                .message
                .clone()
                .unwrap_or_else(|| "Markdown image has no alt text".to_string());
            let mut violation = Violation::new(message)
                .with_path(Arc::<Path>::from(path))
                .with_baseline_key(format!("image\0{}\0{}", image.dest, *occ));
            if let Some(range) = SourceRange::from_byte_range(text, image.span) {
                violation = violation.with_range(range);
            }
            violations.push(violation);
        }
        Ok(violations)
    }
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::PerFile { full_index: false },
    paths: alint_core::PathsSupport::Required,
    scope_filter: alint_core::ScopeFilterSupport::Full,
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::WholeFile,
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
            "markdown_images_have_alt requires a `paths` field",
        ));
    }
    Ok(Box::new(MarkdownImagesHaveAltRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        scope: Scope::from_spec(spec)?,
    }))
}

#[cfg(test)]
mod tests {
    use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

    #[test]
    fn reports_only_live_images_with_empty_rendered_alt_text() {
        let rule = super::build(&spec_yaml(
            "id: alt\nkind: markdown_images_have_alt\npaths: '**/*.md'\nlevel: error\n",
        ))
        .unwrap();
        let text = concat!(
            "![good **description**](good.png)\n",
            "![](bad.png)\n",
            "![   ](also-bad.png)\n",
            "`![](code.png)`\n",
            "<!-- ![](comment.png) -->\n",
        );
        let (tmp, index) = tempdir_with_files(&[("docs/a.md", text.as_bytes())]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].line, Some(2));
        assert_eq!(violations[1].line, Some(3));
        assert!(violations.iter().all(|v| v.range.is_some()));
    }

    #[test]
    fn invalid_utf8_is_a_parse_failure() {
        let rule = super::build(&spec_yaml(
            "id: alt\nkind: markdown_images_have_alt\npaths: '*.md'\nlevel: warning\n",
        ))
        .unwrap();
        let (tmp, index) = tempdir_with_files(&[("a.md", b"![x](y)\xff")]);
        let violations = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].analysis_failure_kind(),
            Some(alint_core::AnalysisFailureKind::ResourceParse)
        );
    }

    #[test]
    fn baseline_key_survives_edits_above_image() {
        let rule = super::build(&spec_yaml(
            "id: alt\nkind: markdown_images_have_alt\npaths: '*.md'\nlevel: warning\n",
        ))
        .unwrap();
        let before = "![](hero.png)\n![](hero.png)\n";
        let after = "# Added heading\n\nExtra prose above.\n\n![](hero.png)\n![](hero.png)\n";
        let (tmp1, idx1) = tempdir_with_files(&[("a.md", before.as_bytes())]);
        let (tmp2, idx2) = tempdir_with_files(&[("a.md", after.as_bytes())]);
        let v1 = rule.evaluate(&ctx(tmp1.path(), &idx1)).unwrap();
        let v2 = rule.evaluate(&ctx(tmp2.path(), &idx2)).unwrap();
        assert_eq!(v1.len(), 2);
        assert_eq!(v2.len(), 2);
        assert_eq!(v1[0].baseline_key, v2[0].baseline_key);
        assert_eq!(v1[1].baseline_key, v2[1].baseline_key);
        assert_ne!(v1[0].baseline_key, v1[1].baseline_key);
    }
}
