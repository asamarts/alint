//! `version: 2` (experimental) at the edges the policy-test corpus cannot
//! reach: a remote `extends:` source (policy tests never fetch one, so the
//! body is seeded in the cache) and the string `alint_dsl::parse` reads. The
//! scenarios under `scenarios/check/version_two/` cover the rest.

use alint_dsl::extends::{Cache, Sri, SriError};

use crate::support;

/// The SRI of `body`, read off the mismatch a zero digest reports.
fn sri_of(body: &str) -> Sri {
    let zero = Sri::parse(&format!("sha256-{}", "0".repeat(64))).unwrap();
    match zero.verify(body.as_bytes()) {
        Err(SriError::Mismatch { actual, .. }) => Sri::parse(&actual).unwrap(),
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

/// Load a `version: 2` config that extends a remote whose body is `remote`.
fn load_extending_remote(remote: &str) -> alint_core::Result<alint_core::Config> {
    let tmp = tempfile::tempdir().unwrap();
    let cache = Cache::at(tmp.path().join("cache"));
    let sri = sri_of(remote);
    cache.put(&sri, remote.as_bytes()).unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        format!(
            "version: 2\nextends: [\"https://example.invalid/rules.yml#{}\"]\nrules: []\n",
            sri.encoded()
        ),
    )
    .unwrap();
    support::load_with(&path, &alint_dsl::LoadOptions::with_cache(cache))
}

#[test]
fn a_remote_source_must_be_version_one() {
    let error = load_extending_remote("version: 2\nrules: []\n")
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "https://example.invalid/rules.yml: a remote `extends:` source must be `version: 1` in this \
         build; `version: 2` is accepted only in local files (the top-level config, a local \
         `extends:` target or an `.alint.d` fragment)"
    );
    let error = load_extending_remote("version: 1\nselectors: {}\nrules: []\n")
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "https://example.invalid/rules.yml: `selectors:` needs `version: 2`, which a remote `extends:` \
         source cannot use in this build: it must be `version: 1`"
    );
    // A version 1 remote under a version 2 config loads.
    let config = load_extending_remote(
        "version: 1\nrules:\n  - id: remote-readme\n    kind: file_exists\n    paths: README.md\n    \
         level: warning\n",
    )
    .unwrap();
    assert_eq!(config.version, 2);
    assert_eq!(config.rules[0].id, "remote-readme");
}

#[test]
fn a_parsed_string_must_be_version_one() {
    assert_eq!(
        alint_dsl::parse("version: 2\nrules: []\n")
            .unwrap_err()
            .to_string(),
        "<inline>: a config parsed from a string (`alint_dsl::parse`) must be `version: 1` in \
         this build; `version: 2` is accepted only in local files (the top-level config, a local \
         `extends:` target or an `.alint.d` fragment)"
    );
}

#[test]
fn version_two_is_listed_as_experimental() {
    assert_eq!(alint_dsl::EXPERIMENTAL_CONFIG_VERSIONS, &[2]);
    assert!(!alint_core::Config::supports_version(2));
}
