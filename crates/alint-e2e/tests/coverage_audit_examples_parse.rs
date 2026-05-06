//! Hard audit: every `.alint.yml` shipped under `/examples/` MUST
//! load + build cleanly via `alint_dsl::load` + `RuleRegistry::build`.
//!
//! The launch-prep validation pass (`docs/launch-prep.md`) ships a
//! growing set of real-repo case studies under `examples/<owner>-<repo>/`.
//! Each case study includes a working `.alint.yml`. The first batch
//! (kubernetes, rust, deno, airflow, turbo) surfaced 12 distinct
//! schema/language pitfalls that wouldn't have shown up without
//! parse-validation — see [`docs/development/CONFIG-AUTHORING.md`]
//! for the full catalogue.
//!
//! This audit is the prevention layer: if a future case study adds
//! a config that re-introduces any of those pitfalls (or any new
//! ones), the test fails at PR time instead of at user-adoption
//! time.
//!
//! What it catches:
//! - `argv:` instead of `command:`, `secondary:` instead of `partner:`,
//!   `style:` instead of `target:`, `pattern:` instead of `prefix:`,
//!   etc. — surfaces as `unknown field` from serde
//! - `timeout: 30s` (string) instead of `timeout: 30` (u64) — surfaces
//!   as a serde type error
//! - Bare-string `fix:` instead of tagged-mapping — surfaces as serde
//!   variant-resolution error
//! - `level:` placed on a nested `for_each_dir.require:` rule instead
//!   of the outer rule — surfaces as `missing field 'level'`
//! - `scope_filter.has_ancestor:` containing a path separator —
//!   surfaces as a custom scope_filter validation error
//! - `when:` / `when_iter:` using `&&`/`!`/method-calls — surfaces
//!   as a when-language parse error
//! - JSONPath dashed-key dot-notation — surfaces as a JSONPath
//!   parse error
//!
//! What it deliberately does NOT catch:
//! - Tool-not-on-PATH errors from `command:` rules — those would
//!   require shellcheck / golangci-lint / etc. to be installed in
//!   CI. The rule structure is correct; the tool absence is an
//!   environment thing. We `build` the rule but don't `evaluate`
//!   against the example repo.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_examples_alint_yml_parses_and_builds() {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples");
    assert!(
        examples_dir.is_dir(),
        "expected examples/ at {}",
        examples_dir.display(),
    );

    let mut configs: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(&examples_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let alint_yml = path.join(".alint.yml");
        if alint_yml.is_file() {
            configs.push(alint_yml);
        }
    }
    assert!(
        !configs.is_empty(),
        "no examples/*/.alint.yml configs found — has the launch-prep \
         validation pass been wiped? (Expected at least 5 from the P2a \
         pilot.)",
    );

    let registry = alint_rules::builtin_registry();
    let mut failures: Vec<String> = Vec::new();

    for config_path in &configs {
        let case_study = config_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");

        // Step 1: load the YAML + resolve `extends:`.
        let config = match alint_dsl::load(config_path) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!(
                    "{case_study}: alint_dsl::load failed — {e}\n  \
                     (path: {})",
                    config_path.display(),
                ));
                continue;
            }
        };

        // Step 2: build every rule via the registry. This is where
        // the schema-level pitfalls in `CONFIG-AUTHORING.md` surface
        // — `unknown field`, `missing field`, type mismatches, etc.
        for spec in &config.rules {
            if matches!(spec.level, alint_core::Level::Off) {
                continue;
            }
            if let Err(e) = registry.build(spec) {
                failures.push(format!(
                    "{case_study}: building rule {:?} failed — {e}\n  \
                     (see docs/development/CONFIG-AUTHORING.md for \
                     common pitfalls)",
                    spec.id,
                ));
            }
            // Step 2b: parse `when:` if present.
            if let Some(when_src) = &spec.when {
                if let Err(e) = alint_core::when::parse(when_src) {
                    failures.push(format!(
                        "{case_study}: rule {:?}: parsing `when:` \
                         failed — {e}\n  (use keywords `and`/`or`/`not`, \
                         not `&&`/`||`/`!`)",
                        spec.id,
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} examples/*/.alint.yml configs failed to load + build:\n\n  - {}\n",
        failures.len(),
        configs.len(),
        failures.join("\n  - "),
    );
}
