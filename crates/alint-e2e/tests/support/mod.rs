use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static REGISTRY: LazyLock<alint_core::RuleRegistry> = LazyLock::new(alint_rules::builtin_registry);

/// Discover implicit policy-test documents without following symlinks.
///
/// This mirrors `alint test` directory discovery: `.yml`/`.yaml` support files
/// without a top-level discriminator and non-UTF-8 YAML fixtures are ignored;
/// malformed documents which do declare `test_version` are retained so the
/// public parser fails loudly.
pub fn policy_test_sources(root: &Path) -> Vec<(PathBuf, String)> {
    let mut sources = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
        {
            let entry = entry.unwrap_or_else(|error| {
                panic!("read entry below {}: {error}", directory.display())
            });
            let path = entry.path();
            let file_type = entry
                .file_type()
                .unwrap_or_else(|error| panic!("read type of {}: {error}", path.display()));
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("yml" | "yaml")
                )
            {
                match std::fs::read_to_string(&path) {
                    Ok(yaml) if alint_testkit::has_policy_test_discriminator(&yaml) => {
                        sources.push((path, yaml));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {}
                    Err(error) => {
                        panic!("read policy-test candidate {}: {error}", path.display());
                    }
                }
            }
        }
    }
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    sources
}

pub fn load(path: &Path) -> alint_core::Result<alint_core::Config> {
    load_with(path, &alint_dsl::LoadOptions::default())
}

pub fn load_with(
    path: &Path,
    options: &alint_dsl::LoadOptions<'_>,
) -> alint_core::Result<alint_core::Config> {
    let options = options
        .clone()
        .with_kind_catalog(Some(REGISTRY.catalog()))
        // Nested-config discovery reads no host Git setup (`walk_options`).
        .with_global_git_excludes(false);
    alint_dsl::load_with(path, &options)
}

/// The walk options of an in-process run: without the user's global excludes
/// file, so what a test's tree holds does not depend on the host's Git setup
/// (a global excludes file that listed `*.yml` hid the files tests wrote).
pub fn walk_options() -> alint_core::WalkOptions {
    alint_core::WalkOptions::default().with_global_git_excludes(false)
}
