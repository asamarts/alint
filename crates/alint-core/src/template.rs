//! String substitution for path templates and message templates.
//!
//! Two variants, distinguished by delimiter style:
//!
//! - **Path templates** — single braces, fixed token set derived from a
//!   matched file's relative path. Example: `"{dir}/{stem}.h"`.
//! - **Message templates** — double braces, namespaced lookups for rule
//!   messages and similar user-facing strings. Example:
//!   `"{{ctx.primary}} has no matching header at {{ctx.partner}}"`.
//!
//! Both are intentionally small and self-contained: no regex dependency,
//! no dynamic parser. Unknown tokens are preserved literally so a typo
//! surfaces in output rather than silently blanking out.

use std::path::Path;

/// Token values derived from a relative path. Consumed by
/// [`render_path`] and by cross-file rules to resolve partner paths.
#[derive(Debug, Clone)]
pub struct PathTokens {
    pub path: String,
    pub dir: String,
    pub basename: String,
    pub stem: String,
    pub ext: String,
    pub parent_name: String,
}

impl PathTokens {
    /// Derive tokens from a relative path. Missing components (e.g. a path
    /// with no parent, or no extension) resolve to the empty string.
    pub fn from_path(rel: &Path) -> Self {
        Self {
            path: rel.display().to_string(),
            dir: rel
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            basename: rel
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            stem: rel
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            ext: rel
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            parent_name: rel
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
}

/// Substitute `{token}` placeholders in a path-shaped template. Unknown
/// tokens are preserved literally (so `"{unknown}"` renders as `"{unknown}"`).
///
/// Multi-character tokens are replaced longest-first so future additions like
/// `{stem_kebab}` do not accidentally match `{stem}` first.
pub fn render_path(template: &str, t: &PathTokens) -> String {
    let mut out = template.to_string();
    // Order matters: longest keys first.
    out = out.replace("{parent_name}", &t.parent_name);
    out = out.replace("{basename}", &t.basename);
    out = out.replace("{path}", &t.path);
    out = out.replace("{stem}", &t.stem);
    out = out.replace("{dir}", &t.dir);
    out = out.replace("{ext}", &t.ext);
    out
}

/// Substitute `{{namespace.key}}` placeholders in a message template. The
/// caller-supplied `resolve` closure returns the substituted value, or
/// `None` to leave the placeholder literal.
///
/// Whitespace inside the braces (`{{ ctx.primary }}`) is ignored so users
/// can format their messages for readability.
pub fn render_message<F>(template: &str, resolve: F) -> String
where
    F: Fn(&str, &str) -> Option<String>,
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            // Unterminated {{ — preserve rest literally.
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = after[..end].trim();
        let rendered = inner
            .split_once('.')
            .and_then(|(ns, key)| resolve(ns.trim(), key.trim()));
        if let Some(val) = rendered {
            out.push_str(&val);
        } else {
            out.push_str("{{");
            out.push_str(&after[..end]);
            out.push_str("}}");
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn path_tokens_basic_rs_file() {
        let t = PathTokens::from_path(Path::new("crates/alint-core/src/lib.rs"));
        assert_eq!(t.path, "crates/alint-core/src/lib.rs");
        assert_eq!(t.dir, "crates/alint-core/src");
        assert_eq!(t.basename, "lib.rs");
        assert_eq!(t.stem, "lib");
        assert_eq!(t.ext, "rs");
        assert_eq!(t.parent_name, "src");
    }

    #[test]
    fn path_tokens_root_file() {
        let t = PathTokens::from_path(Path::new("README.md"));
        assert_eq!(t.path, "README.md");
        assert_eq!(t.dir, "");
        assert_eq!(t.basename, "README.md");
        assert_eq!(t.stem, "README");
        assert_eq!(t.ext, "md");
        assert_eq!(t.parent_name, "");
    }

    #[test]
    fn render_path_c_to_h() {
        let t = PathTokens::from_path(Path::new("src/mod/foo.c"));
        assert_eq!(render_path("{dir}/{stem}.h", &t), "src/mod/foo.h");
    }

    #[test]
    fn render_path_unknown_token_preserved() {
        let t = PathTokens::from_path(Path::new("a.c"));
        assert_eq!(render_path("{bogus}/{stem}.x", &t), "{bogus}/a.x");
    }

    #[test]
    fn render_message_simple() {
        let out = render_message("{{ctx.primary}} → {{ctx.partner}}", |ns, key| {
            match (ns, key) {
                ("ctx", "primary") => Some("a.c".into()),
                ("ctx", "partner") => Some("a.h".into()),
                _ => None,
            }
        });
        assert_eq!(out, "a.c → a.h");
    }

    #[test]
    fn render_message_ignores_inner_whitespace() {
        let out = render_message("[{{ ctx . primary }}]", |ns, key| {
            if ns == "ctx" && key == "primary" {
                Some("x".into())
            } else {
                None
            }
        });
        assert_eq!(out, "[x]");
    }

    #[test]
    fn render_message_unknown_key_preserved() {
        let out = render_message("{{ctx.unknown}}", |_, _| None);
        assert_eq!(out, "{{ctx.unknown}}");
    }

    #[test]
    fn render_message_unterminated_is_preserved() {
        let out = render_message("before {{ctx.primary", |_, _| Some("X".into()));
        assert_eq!(out, "before {{ctx.primary");
    }

    #[test]
    fn render_message_no_placeholders() {
        let out = render_message("plain text", |_, _| Some("never".into()));
        assert_eq!(out, "plain text");
    }
}
