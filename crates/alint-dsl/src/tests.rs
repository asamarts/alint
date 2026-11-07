use super::*;

mod hardening;
mod loading;
mod nested_config;
mod remote_trust;

fn test_registry() -> &'static alint_core::RuleRegistry {
    static REGISTRY: std::sync::LazyLock<alint_core::RuleRegistry> =
        std::sync::LazyLock::new(|| {
            let mut registry = alint_core::RuleRegistry::new();
            alint_rules::register_builtin(&mut registry);
            registry
        });
    &REGISTRY
}

fn test_catalog() -> alint_core::KindCatalog<'static> {
    test_registry().catalog()
}

fn load(path: &std::path::Path) -> alint_core::Result<Config> {
    super::load_with(path, &LoadOptions::with_catalog(test_catalog()))
}

fn load_with(path: &std::path::Path, opts: &LoadOptions<'_>) -> alint_core::Result<Config> {
    let mut opts = opts.clone();
    opts.kind_catalog = Some(test_catalog());
    super::load_with(path, &opts)
}

// --- W2 content-fixer trust (auto-fix.md 5.5) ---------------------------------

/// The applicability a loaded rule's content fixer DECLARES (`None` = unset, so the
/// builder's default tier applies). W2 sets it to `Suggestion` for a content fixer
/// from an untrusted remote.
fn declared_content_tier(rule: &alint_core::RuleSpec) -> Option<alint_core::Applicability> {
    use alint_core::FixSpec;
    match rule.fix.as_ref()? {
        FixSpec::Replace { replace } => replace.applicability,
        FixSpec::FileCreate { file_create } => file_create.applicability,
        FixSpec::FilePrepend { file_prepend } => file_prepend.applicability,
        FixSpec::FileAppend { file_append } => file_append.applicability,
        FixSpec::SetValue { set_value } => set_value.applicability,
        FixSpec::RemoveValue { remove_value } => remove_value.applicability,
        FixSpec::SyncFrom { sync_from } => sync_from.applicability,
        FixSpec::CreateAndRegister {
            create_and_register,
        } => create_and_register.applicability,
        // `relocate` is fixed-behavior (never demoted), but reading its declared
        // tier explicitly gives `w2_remote_relocate_is_not_demoted` teeth: a
        // regression that demoted it would surface here as `Some(Suggestion)`.
        FixSpec::Relocate { relocate } => relocate.applicability,
        // `sort` is content-injecting (demoted from an untrusted remote); read its
        // tier so `w2_remote_sort_is_demoted_to_suggestion` sees the cap.
        FixSpec::Sort { sort } => sort.applicability,
        // `indent_style` is content-injecting for the same reason (aim a reindent
        // at a Makefile); read its tier for `w2_remote_indent_style_is_demoted`.
        FixSpec::IndentStyle { indent_style } => indent_style.applicability,
        // `insert_line` injects the host rule's `require:` lines; read its tier for
        // `w2_remote_insert_line_is_demoted`.
        FixSpec::InsertLine { insert_line } => insert_line.applicability,
        // `insert_header` injects the host `file_header` rule's header bytes; read
        // its tier for `w2_remote_insert_header_is_demoted`.
        FixSpec::InsertHeader { insert_header } => insert_header.applicability,
        // `file_rename` / `file_normalize_line_endings` are aimable Safe transforms
        // (case-rename breaks imports; CRLF breaks a shebang); read their tier for
        // `w2_remote_file_rename_is_demoted` / `..._file_normalize_line_endings_...`.
        FixSpec::FileRename { file_rename } => file_rename.applicability,
        FixSpec::FileNormalizeLineEndings {
            file_normalize_line_endings,
        } => file_normalize_line_endings.applicability,
        // The hygiene normalizers + `chmod` are content-injecting too (aimable at the
        // Safe tier -- R3); read each tier for its `w2_remote_<op>_is_demoted` test.
        // `file_strip_bidi` / `file_strip_zero_width` are NOT here -- they stay
        // fixed-behavior (security-positive), covered by
        // `w2_remote_security_positive_strip_is_not_demoted`.
        FixSpec::FileTrimTrailingWhitespace {
            file_trim_trailing_whitespace,
        } => file_trim_trailing_whitespace.applicability,
        FixSpec::FileAppendFinalNewline {
            file_append_final_newline,
        } => file_append_final_newline.applicability,
        FixSpec::FileStripBom { file_strip_bom } => file_strip_bom.applicability,
        FixSpec::FileCollapseBlankLines {
            file_collapse_blank_lines,
        } => file_collapse_blank_lines.applicability,
        FixSpec::Chmod { chmod } => chmod.applicability,
        _ => None,
    }
}

/// Seed `cache` with `body` under its own SRI and return the `https://…#sha256-…`
/// URL a config would `extends:`. Its base (no fragment) is
/// `https://example.invalid/remote.yml`.
fn seed_remote(cache: &extends::Cache, body: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(body.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in &digest {
        use std::fmt::Write as _;
        write!(hex, "{b:02x}").unwrap();
    }
    let sri_str = format!("sha256-{hex}");
    let sri = extends::Sri::parse(&sri_str).unwrap();
    cache.put(&sri, body.as_bytes()).unwrap();
    format!("https://example.invalid/remote.yml#{sri_str}")
}

const REMOTE_REPLACE: &str = "version: 1\nrules:\n  - id: no-todo\n    \
     kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: TODO\n    \
     level: error\n    fix: { replace: { replacement: DONE } }\n";

fn load_extending(remote_body: &str, top_extra: &str) -> alint_core::Config {
    load_extending_top(
        remote_body,
        &format!("version: 1\n{{url}}\n{top_extra}rules: []\n"),
    )
}

/// Load an exact top-level body around a seeded remote. `{url}` is replaced by
/// the generated `extends:` line, allowing tests that need top-level rules rather
/// than the empty list used by [`load_extending`].
fn load_extending_top(remote_body: &str, top_body: &str) -> alint_core::Config {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, remote_body);
    let config_path = tmp.path().join(".alint.yml");
    let extends_line = format!("extends: [\"{url}\"]");
    std::fs::write(&config_path, top_body.replace("{url}", &extends_line)).unwrap();
    // Keep the tempdir alive for the duration of the load by leaking it into the
    // cache path (the cache is read during load); simplest is to load before drop.
    let opts = LoadOptions::with_cache(cache);
    let cfg = load_with(&config_path, &opts).unwrap();
    drop(tmp);
    cfg
}

/// `Config::new` gives every field the value a file that sets only `version`
/// and `rules` loads with.
#[test]
fn config_new_matches_a_minimal_loaded_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".alint.yml");
    std::fs::write(&path, "version: 1\nrules: []\n").unwrap();
    let loaded = load(&path).unwrap();
    let built = Config::new(1, Vec::new());
    assert_eq!(built.version, loaded.version);
    assert_eq!(built.requires.is_none(), loaded.requires.is_none());
    assert_eq!(built.extends.len(), loaded.extends.len());
    assert_eq!(built.ignore, loaded.ignore);
    assert_eq!(built.respect_gitignore, loaded.respect_gitignore);
    assert_eq!(built.vars, loaded.vars);
    assert_eq!(built.facts.len(), loaded.facts.len());
    assert_eq!(built.rules.len(), loaded.rules.len());
    assert_eq!(built.fix_size_limit, loaded.fix_size_limit);
    assert_eq!(built.nested_configs, loaded.nested_configs);
    assert_eq!(
        format!("{:?}", built.allow_out_of_root),
        format!("{:?}", loaded.allow_out_of_root)
    );
    assert_eq!(built.baseline, loaded.baseline);
    assert!(built.environment_values.is_empty() && loaded.environment_values.is_empty());
}
