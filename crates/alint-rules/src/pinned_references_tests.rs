use super::*;
use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn run(path: &str, text: &str, format: Option<&str>) -> Vec<Violation> {
    let format = format.map_or_else(String::new, |value| format!("format: {value}\n"));
    let spec = spec_yaml(&format!(
        "id: pins\nkind: pinned_references\npaths: {path:?}\nlevel: error\n{format}"
    ));
    let rule = build(&spec).unwrap();
    let (tmp, index) = tempdir_with_files(&[(path, text.as_bytes())]);
    rule.evaluate(&ctx(tmp.path(), &index)).unwrap()
}

fn digest(image: &str) -> String {
    format!("{image}@sha256:{SHA256}")
}

#[test]
fn pin_predicates_are_exact() {
    assert!(is_full_git_oid(SHA1));
    assert!(is_full_git_oid(SHA256));
    assert!(!is_full_git_oid(&SHA1[..39]));
    assert!(!is_full_git_oid(&format!("{}g", &SHA1[..39])));
    assert!(git_reference_is_pinned(&format!("owner/repo@{SHA1}")));
    assert!(!git_reference_is_pinned("owner/repo@v4"));
    assert!(image_is_pinned(&digest("registry:5000/image:tag")));
    assert!(image_is_pinned(&digest("[2001:db8::1]:5000/team/image")));
    assert!(!image_is_pinned("registry:5000/image:tag"));
    assert!(!image_is_pinned(&format!(
        "image@sha256:{}",
        SHA256.to_ascii_uppercase()
    )));
    assert!(!git_reference_is_pinned(&format!(
        "${{{{ matrix.action }}}}@{SHA1}"
    )));
    assert!(!image_is_pinned(&digest("$CI_IMAGE")));
    assert!(orb_reference_is_pinned("circleci/node@5.2.0"));
    assert!(orb_reference_is_pinned("owner/orb@1.2.3-rc.1+build.4"));
    for value in [
        "circleci/node@5",
        "circleci/node@5.2",
        "circleci/node@volatile",
        "circleci/node@dev:main",
        "circleci/node@1.2.x",
        "<< pipeline.parameters.orb >>@1.2.3",
    ] {
        assert!(!orb_reference_is_pinned(value), "{value}");
    }
    assert!(orb_reference_is_pinned("circleci/node@0.0.0"));
    assert!(orb_reference_is_pinned("circleci/node@1.2.3+01"));
    for value in [
        "circleci/node@01.2.3",
        "circleci/node@1.02.3",
        "circleci/node@1.2.03",
        "circleci/node@1.2.3-01",
    ] {
        assert!(!orb_reference_is_pinned(value), "{value}");
    }
}

#[test]
fn templated_locators_do_not_pass_with_immutable_suffixes() {
    let action =
        format!("jobs:\n  build:\n    steps:\n      - uses: ${{{{ matrix.action }}}}@{SHA1}\n");
    assert_eq!(run(".github/workflows/ci.yml", &action, None).len(), 1);

    let image = format!("job:\n  image: $CI_IMAGE@sha256:{SHA256}\n");
    assert_eq!(run(".gitlab-ci.yml", &image, None).len(), 1);

    let orb = "version: 2.1\norbs:\n  tool: << pipeline.parameters.orb >>@1.2.3\n";
    assert_eq!(run(".circleci/config.yml", orb, None).len(), 1);
}

#[test]
fn auto_detects_every_supported_path() {
    for (path, expected) in [
        (".github/workflows/ci.yml", InputFormat::GithubActions),
        ("tools/action.yaml", InputFormat::GithubActions),
        (".pre-commit-config.yaml", InputFormat::PreCommit),
        (".circleci/config.yml", InputFormat::Circleci),
        (".gitlab-ci.yml", InputFormat::GitlabCi),
        ("cloudbuild.yaml", InputFormat::CloudBuild),
        ("docker/Dockerfile.test", InputFormat::Dockerfile),
    ] {
        assert!(matches!(
            (detect_format(Path::new(path)), expected),
            (Some(InputFormat::GithubActions), InputFormat::GithubActions)
                | (Some(InputFormat::PreCommit), InputFormat::PreCommit)
                | (Some(InputFormat::Circleci), InputFormat::Circleci)
                | (Some(InputFormat::GitlabCi), InputFormat::GitlabCi)
                | (Some(InputFormat::CloudBuild), InputFormat::CloudBuild)
                | (Some(InputFormat::Dockerfile), InputFormat::Dockerfile)
        ));
    }
    let findings = run("ci/custom.yml", "steps: []\n", None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::Evaluation)
    );
}

#[test]
fn github_covers_workflows_reusable_jobs_composites_and_images() {
    let source = format!(
        "jobs:\n  reusable:\n    uses: owner/repo/.github/workflows/ci.yml@v1\n  build:\n    container: ubuntu:24.04\n    services:\n      db:\n        image: postgres:16\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./local-action\n      - uses: owner/repo@{SHA1}\n      - uses: docker://alpine:3\n      - uses: docker://scratch\nruns:\n  using: composite\n  steps:\n    - uses: owner/another@main\n"
    );
    let findings = run(".github/workflows/ci.yml", &source, None);
    assert_eq!(findings.len(), 6);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(3), Some(5), Some(8), Some(10), Some(13), Some(18)]
    );
    assert!(findings.iter().all(|finding| finding.range.is_some()));
    assert!(
        findings
            .iter()
            .all(|finding| !finding.message.contains("owner/repo"))
    );
}

#[test]
fn github_workflow_steps_preserve_the_legacy_baseline_identity() {
    let findings = run(
        ".github/workflows/ci.yml",
        "jobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n",
        None,
    );
    assert_eq!(findings.len(), 1);
    let expected = crate::structured_path::matches_baseline_key(
        LEGACY_GITHUB_STEPS_PATH,
        LEGACY_GITHUB_PIN_REGEX,
        &serde_json::json!("actions/checkout@v4"),
    );
    assert_eq!(findings[0].baseline_key.as_deref(), Some(expected.as_str()));
}
#[test]
fn github_preserves_plain_quoted_and_unicode_scalar_ranges() {
    let source = "jobs:\n  build:\n    steps:\n      - uses: 'owner/über@main'\n      - uses: owner/plain@v1\n";
    let findings = run("action.yml", source, None);
    assert_eq!(findings.len(), 2);
    let first = findings[0].range.as_ref().unwrap();
    assert_eq!(
        (
            first.start_line(),
            first.start_column(),
            first.end_line(),
            first.end_column(),
        ),
        (4, 15, 4, 32)
    );
    let second = findings[1].range.as_ref().unwrap();
    assert_eq!(
        (
            second.start_line(),
            second.start_column(),
            second.end_line(),
            second.end_column(),
        ),
        (5, 15, 5, 29)
    );
}

#[test]
fn github_accepts_full_pins_and_local_references() {
    let source = format!(
        "jobs:\n  reusable:\n    uses: owner/repo/.github/workflows/ci.yml@{SHA256}\n  build:\n    container: {}\n    services:\n      db:\n        image: {}\n    steps:\n      - uses: actions/checkout@{SHA1}\n      - uses: ./local\n      - uses: docker://{}\nruns:\n  steps:\n    - uses: ./child\n",
        digest("ubuntu:24.04"),
        digest("postgres:16"),
        digest("alpine:3")
    );
    assert!(run("action.yml", &source, None).is_empty());
}

#[test]
fn github_workflow_step_keeps_the_legacy_baseline_identity() {
    let findings = run(
        ".github/workflows/ci.yml",
        "jobs:\n  build:\n    steps:\n      - uses: owner/repo@main\n",
        None,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].baseline_key.as_deref(),
        Some(
            "$.jobs.*.steps[*].uses\0=~ ^(\\./.*|docker://[^@]+@sha256:[a-f0-9]{64}|[a-zA-Z0-9._/-]+@[a-f0-9]{40})$\0got \"owner/repo@main\""
        )
    );
}

#[test]
fn pre_commit_checks_revisions_and_exempts_local_repositories() {
    let source = format!(
        "repos:\n  - repo: https://example.test/one\n    rev: v1.2.3\n  - repo: https://example.test/two\n  - repo: local\n  - repo: meta\n  - repo: https://example.test/pinned\n    rev: {SHA1}\n"
    );
    let findings = run(".pre-commit-config.yaml", &source, None);
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].line, Some(3));
    assert_eq!(findings[1].line, Some(4));
    assert!(findings[1].message.contains("no pinned `rev`"));
}

#[test]
fn circleci_checks_exact_orbs_and_job_and_executor_images() {
    let source = "version: 2.1\norbs:\n  node: circleci/node@volatile\n  browser: circleci/browser-tools@1.4.0\njobs:\n  test:\n    docker:\n      - image: cimg/rust:1.90\nexecutors:\n  default:\n    docker:\n      - image: cimg/base:current\n";
    let findings = run(".circleci/config.yml", source, None);
    assert_eq!(findings.len(), 3);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(3), Some(8), Some(12)]
    );
}
#[test]
fn gitlab_checks_root_mapping_services_and_merged_jobs_once_per_source() {
    let source = format!(
        "image: root:latest\nservices:\n  - root-service:latest\ndefault:\n  image:\n    name: ruby:3.3\n  services:\n    - name: {}\n.template: &template\n  image: alpine:latest\nbuild:\n  <<: *template\n  services:\n    - postgres:16\npinned:\n  image: {}\n",
        digest("redis:7"),
        digest("debian:bookworm")
    );
    let findings = run(".gitlab-ci.yml", &source, None);
    assert_eq!(findings.len(), 5);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(1), Some(3), Some(6), Some(12), Some(14)]
    );
}
#[test]
fn explicit_keys_override_merged_mutable_values() {
    let source = format!(
        "jobs:\n  build:\n    <<: &defaults\n      container: ubuntu:latest\n    container: {}\n",
        digest("ubuntu:24.04")
    );
    assert!(run(".github/workflows/ci.yml", &source, None).is_empty());
}

#[test]
fn yaml_dynamic_maps_expand_merges_and_apply_last_key_wins() {
    let source = format!(
        "definitions: &definitions\n  inherited:\n    steps:\n      - uses: owner/inherited@main\njobs:\n  <<: *definitions\n  replaced:\n    steps:\n      - uses: owner/old@main\n  replaced:\n    steps:\n      - uses: owner/new@{SHA1}\n"
    );
    let findings = run(".github/workflows/ci.yml", &source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(6));
}

#[test]
fn yaml_container_aliases_report_each_use_site() {
    let source = "shared_steps: &shared_steps\n  - uses: owner/action@main\njob: &job\n  container: ubuntu:latest\njobs:\n  one:\n    steps: *shared_steps\n  two:\n    steps: *shared_steps\n  three: *job\n  four: *job\n";
    let findings = run(".github/workflows/ci.yml", source, None);
    assert_eq!(findings.len(), 4);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(7), Some(9), Some(10), Some(11)]
    );
}

#[test]
fn yaml_merge_sequence_uses_first_mapping_then_explicit_keys() {
    let source = format!(
        "pinned: &pinned\n  container: {}\nmutable: &mutable\n  container: ubuntu:latest\njobs:\n  build:\n    <<: [*pinned, *mutable]\n",
        digest("ubuntu:24.04")
    );
    assert!(run(".github/workflows/ci.yml", &source, None).is_empty());
}

#[test]
fn gitlab_pages_are_jobs_and_hidden_templates_only_fire_when_used() {
    let source = format!(
        ".template: &template\n  image: alpine:latest\nsafe:\n  <<: *template\n  image: {}\npages:\n  image: ruby:latest\n",
        digest("alpine:3")
    );
    let findings = run(".gitlab-ci.yml", &source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(7));
}

#[test]
fn gitlab_resolves_extends_and_reference_with_source_locations() {
    let source = ".base:\n  image: alpine:latest\n  services:\n    - postgres:16\n.override:\n  image: ruby:3.3\none:\n  extends: .base\ntwo:\n  extends: [.base, .override]\nthree:\n  image: !reference [.base, image]\n  services: !reference [.base, services]\n";
    let findings = run(".gitlab-ci.yml", source, None);
    assert_eq!(findings.len(), 6);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(2), Some(2), Some(4), Some(4), Some(4), Some(6)]
    );
    assert!(findings.iter().all(|finding| finding.range.is_some()));
}
#[test]
fn quoted_merge_key_remains_a_gitlab_job_name() {
    let findings = run(
        ".gitlab-ci.yml",
        "\"<<\":\n  image: alpine:latest\n  script: echo test\n",
        None,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(2));
}

#[test]
fn explicit_yaml_string_tags_do_not_become_null_or_merge_keys() {
    let findings = run(
        ".gitlab-ci.yml",
        "!!str <<:\n  image: alpine:latest\njob:\n  image: !!str null\n",
        None,
    );
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(2), Some(4)]
    );
}

#[test]
fn gitlab_resolves_reference_tags_nested_in_image_mappings() {
    let source = ".images:\n  mutable: alpine:latest\n  pinned: alpine@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\njob:\n  image:\n    name: !reference [.images, mutable]\n  services:\n    - name: !reference [.images, mutable]\nsafe:\n  image:\n    name: !reference [.images, pinned]\n";
    let findings = run(".gitlab-ci.yml", source, None);
    assert_eq!(findings.len(), 2);
    assert!(
        findings
            .iter()
            .all(|finding| finding.line == Some(2) && finding.range.is_some())
    );
}

#[test]
fn gitlab_include_owned_references_remain_unresolved_without_failing() {
    for source in [
        "include:\n  project: shared/pipeline\n  file: /base.yml\njob:\n  extends: .remote\n",
        "include: shared.yml\njob:\n  image: !reference [.remote, image]\n",
    ] {
        assert!(run(".gitlab-ci.yml", source, None).is_empty(), "{source:?}");
    }
}

#[test]
fn gitlab_null_removes_inherited_images_and_services() {
    let source = ".base:\n  image: alpine:latest\n  services: [postgres:16]\njob:\n  extends: .base\n  image: null\n  services: ~\n";
    assert!(run(".gitlab-ci.yml", source, None).is_empty());
}

#[test]
fn gitlab_inheritance_errors_fail_loud_without_values() {
    for source in [
        ".one:\n  extends: .two\n.two:\n  extends: .one\njob:\n  extends: .one\n",
        "job:\n  extends: .missing\n",
        ".base:\n  image: alpine:latest\njob:\n  image: !reference [.missing, image]\n",
    ] {
        let findings = run(".gitlab-ci.yml", source, None);
        assert_eq!(findings.len(), 1, "{source:?}");
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse),
            "{source:?}"
        );
        assert!(!findings[0].message.contains("alpine:latest"));
    }
}

#[test]
fn duplicate_yaml_keys_use_the_last_explicit_value() {
    let source = format!(
        "job:\n  image: alpine:latest\n  image: {}\n",
        digest("alpine:3")
    );
    assert!(run(".gitlab-ci.yml", &source, None).is_empty());
}

#[test]
fn cloud_build_checks_only_step_images() {
    let source = format!(
        "steps:\n  - name: gcr.io/cloud-builders/docker:latest\n  - name: {}\nimages:\n  - output/image:latest\n",
        digest("gcr.io/project/builder:tag")
    );
    let findings = run("cloudbuild.yaml", &source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(2));
}

#[test]
fn yaml_alias_use_has_the_alias_location() {
    let source = "image: &image alpine:latest\nsteps:\n  - name: *image\n";
    let findings = run("custom.yml", source, Some("cloud_build"));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(3));
    assert_eq!(findings[0].column, Some(11));
}

#[test]
fn malformed_non_utf8_and_multi_document_yaml_fail_loud() {
    let findings = run(".gitlab-ci.yml", "jobs: [\n", None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );
    let findings = run(".gitlab-ci.yml", "---\na: b\n---\nc: d\n", None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );

    let spec =
        spec_yaml("id: pins\nkind: pinned_references\npaths: .gitlab-ci.yml\nlevel: error\n");
    let rule = build(&spec).unwrap();
    let (tmp, index) = tempdir_with_files(&[(".gitlab-ci.yml", &[0xff, 0xfe])]);
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );
}

#[test]
fn yaml_safety_limits_fail_loud_before_tree_construction() {
    let nested = format!("jobs: {}x{}\n", "[".repeat(129), "]".repeat(129));
    let findings = run(".github/workflows/ci.yml", &nested, None);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );

    let alias_bomb = format!(
        "value: &large {}\nservices:\n{}",
        "x".repeat(1_000_000),
        "  - *large\n".repeat(300)
    );
    let findings = run(".gitlab-ci.yml", &alias_bomb, None);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );
}

#[test]
fn empty_and_comment_only_yaml_are_empty_documents() {
    assert!(run(".gitlab-ci.yml", "", None).is_empty());
    assert!(run(".gitlab-ci.yml", "# no configuration\n", None).is_empty());
}

#[test]
fn parser_failures_do_not_echo_unrelated_values() {
    for (path, source) in [
        (".gitlab-ci.yml", "jobs: [TOP_SECRET\n"),
        ("Dockerfile", "RUN <<TOP_SECRET\nnever closed\n"),
    ] {
        let findings = run(path, source, None);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
        assert!(!findings[0].message.contains("TOP_SECRET"));
    }
}

#[test]
fn oversized_inputs_fail_before_format_parsing() {
    let spec = spec_yaml("id: pins\nkind: pinned_references\npaths: Dockerfile\nlevel: error\n");
    let rule = build(&spec).unwrap();
    let (tmp, index) = tempdir_with_files(&[]);
    let bytes = vec![b'x'; MAX_PINNED_REFERENCES_BYTES + 1];
    let findings = rule
        .as_per_file()
        .unwrap()
        .evaluate_file(&ctx(tmp.path(), &index), Path::new("Dockerfile"), &bytes)
        .unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );
}

#[test]
fn dockerfile_checks_from_and_remote_add_across_forms() {
    let source = format!(
        "# syntax=docker/dockerfile:1\nFROM --platform=$BUILDPLATFORM rust:1.90 AS build\nFROM {}\nFROM scratch\nADD https://example.test/tool /usr/bin/tool\nADD --checksum=sha256:{SHA256} https://example.test/pinned /tmp/\nADD [\"https://example.test/archive\", \"/src/\"]\nADD local.txt /src/\n",
        digest("alpine:3")
    );
    let findings = run("Dockerfile", &source, None);
    assert_eq!(findings.len(), 3);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(2), Some(5), Some(7)]
    );
    let variables = run("Dockerfile", "FROM ${BASE_IMAGE}\n", None);
    assert_eq!(variables.len(), 1);
}

#[test]
fn dockerfile_honors_continuations_escape_directive_and_onbuild() {
    let source = format!(
        "# escape=`\nFROM `\n  {} AS base\nONBUILD ADD `\n  --checksum=sha256:{SHA256} `\n  https://example.test/tool /tool\nFROM ubuntu:24.04\n",
        digest("alpine:3")
    );
    let findings = run("Dockerfile.windows", &source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(7));
    let range = findings[0].range.as_ref().unwrap();
    assert_eq!(range.start_line(), 7);
    assert_eq!(range.end_line(), 7);
}

#[test]
fn dockerfile_uses_only_the_initial_parser_directive_block() {
    let source = format!(
        "# ordinary comment\n# escape=`\nFROM \\\n  {}\n",
        digest("alpine:3")
    );
    assert!(run("Dockerfile", &source, None).is_empty());
}

#[test]
fn dockerfile_accepts_prior_build_stages_and_flags_variable_add_sources() {
    let source = format!(
        "FROM {} AS build\nFROM build AS test\nADD $URL /one\nADD [\"${{ARCHIVE_URL}}\", \"/two\"]\nADD --checksum=sha256:{SHA256} $PINNED_URL /three\n",
        digest("rust:1.90")
    );
    let findings = run("Dockerfile", &source, None);
    assert_eq!(findings.len(), 3);
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.line)
            .collect::<Vec<_>>(),
        [Some(3), Some(4), Some(5)]
    );
}

#[test]
fn dockerfile_stage_names_and_literal_dollars_follow_consumer_semantics() {
    let source = format!(
        "FROM {} AS BUILD\nFROM build\nFROM SCRATCH\nADD \\$LOCAL_ARCHIVE /src/\nADD '$LOCAL_ARCHIVE' /src/\nADD \"$REMOTE_URL\" /src/\n",
        digest("rust:1.90")
    );
    let findings = run("Dockerfile", &source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(6));
}

#[test]
fn dockerfile_skips_heredoc_bodies_and_fails_on_unterminated_ones() {
    let source = "RUN <<EOF\nFROM hidden:latest\nADD https://hidden.test/file /tmp/\nEOF\nFROM visible:latest\n";
    let findings = run("Dockerfile", source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(5));

    let findings = run("Dockerfile", "RUN <<EOF\nnever closed\n", None);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );
}

#[test]
fn dockerfile_rejects_excessive_physical_lines_before_collecting_them() {
    let source = "#\n".repeat(MAX_DOCKERFILE_PHYSICAL_LINES + 1);
    let error = dockerfile_instructions(&source).unwrap_err();
    assert!(error.contains("line safety limit"), "{error}");
}

#[test]
fn dockerfile_honors_quoted_tab_stripped_and_multiple_heredocs() {
    let source = "RUN <<-'ONE' <<\"TWO\"\n\tFROM hidden-one:latest\n\tONE\nADD https://hidden-two.test/file /tmp/\nTWO\nFROM visible:latest\n";
    let findings = run("Dockerfile", source, None);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, Some(6));
}
#[test]
fn dockerfile_directives_are_case_insensitive_and_malformed_arguments_fail_loud() {
    let source = format!(
        "# ESCAPE = `\nFROM `\n  {image}\n",
        image = digest("alpine")
    );
    assert!(run("Dockerfile", &source, None).is_empty());

    for source in [
        "FROM \"unterminated\n",
        "FROM\n",
        "ADD only-a-source\n",
        "ONBUILD\n",
        "ADD --from=build local /dest\n",
    ] {
        let findings = run("Dockerfile", source, None);
        assert_eq!(findings.len(), 1, "{source:?}");
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse),
            "{source:?}"
        );
    }

    for source in [
        "# escape=x\nFROM alpine:latest\n",
        "# escape=\\\n# escape=`\nFROM alpine:latest\n",
        "# syntax=docker/dockerfile:1\n# syntax=docker/dockerfile:1.1\nFROM alpine:latest\n",
    ] {
        let findings = run("Dockerfile", source, None);
        assert_eq!(findings.len(), 1, "{source:?}");
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse),
            "{source:?}"
        );
    }
}

#[test]
fn explicit_format_and_custom_message_preserve_per_reference_findings() {
    let spec = spec_yaml(
        "id: pins\nkind: pinned_references\npaths: custom.yaml\nformat: cloud_build\nmessage: Pin it\nlevel: error\n",
    );
    let rule = build(&spec).unwrap();
    let (tmp, index) = tempdir_with_files(&[(
        "custom.yaml",
        b"steps:\n  - name: one:latest\n  - name: two:latest\n",
    )]);
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    assert_eq!(findings.len(), 2);
    assert!(findings.iter().all(|finding| finding.message == "Pin it"));
    assert_ne!(findings[0].baseline_key, findings[1].baseline_key);
}

#[test]
fn builder_rejects_missing_paths_unknown_options_and_fix() {
    assert!(build(&spec_yaml("id: p\nkind: pinned_references\nlevel: error\n")).is_err());
    assert!(
        build(&spec_yaml(
            "id: p\nkind: pinned_references\npaths: x\nunknown: true\nlevel: error\n"
        ))
        .is_err()
    );
    assert!(
        build(&spec_yaml(
            "id: p\nkind: pinned_references\npaths: x\nfix:\n  file_remove: {}\nlevel: error\n"
        ))
        .is_err()
    );
}

/// Run `job` on a thread of its own, and fail unless it returns within `limit`
/// (a hang leaves the thread spinning, but fails the test).
fn within<T: Send + 'static>(
    limit: std::time::Duration,
    job: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || drop(sender.send(job())));
    receiver
        .recv_timeout(limit)
        .unwrap_or_else(|_| panic!("did not finish within {limit:?}"))
}

/// `prefix`, then a workflow whose `build` job merges `m{depth}`, each `m`
/// merging the one before `fanout` times: an unbounded lookup of a key that is
/// not there visits `fanout^depth` merge sources.
fn merge_chain(prefix: &str, depth: usize, fanout: usize) -> String {
    use std::fmt::Write as _;

    let mut text =
        format!("{prefix}m1: &m1 {{runs-on: x, steps: [{{uses: actions/checkout@v4}}]}}\n");
    for level in 2..=depth {
        let sources = vec![format!("*m{}", level - 1); fanout].join(", ");
        writeln!(text, "m{level}: &m{level} {{<<: [{sources}]}}").unwrap();
    }
    writeln!(text, "jobs: {{build: {{<<: *m{depth}}}}}").unwrap();
    text
}

fn github_rule() -> PinnedReferencesRule {
    let spec = spec_yaml("id: pins\nkind: pinned_references\npaths: ci.yml\nlevel: error\n");
    PinnedReferencesRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: None,
        message: None,
        scope: Scope::from_spec(&spec).unwrap(),
        format: InputFormat::GithubActions,
    }
}

/// Texts ayaml refuses, as the alias guard measures with it, but saphyr reads:
/// an alias to a collection that contains it, and an empty verbatim tag.
const AYAML_ONLY_ERRORS: &[&str] = &["z: &z [*z]\n", "z: &z {k: [*z]}\n", "x: !<> y\n"];

/// ROB-1: the alias guard took any refusal but its own budget for "within
/// budget", and saphyr then read what ayaml refused: a merge source that
/// contains its own merge key, or a merge chain after an ayaml-only error,
/// spun the lookups for 2^128 or 10^12 steps.
#[test]
fn merge_aliases_ayaml_cannot_read_are_refused_promptly() {
    let workflow = "name: ci\non: push\njobs:\n  a: &r\n    <<: [*r, *r]\n";
    let minimal = "&r\n<<: [*r, *r]\n";
    let chain = merge_chain("z: &z [*z]\n", 12, 10);
    assert!(chain.len() < 1024, "{}", chain.len());
    for text in [workflow, minimal, chain.as_str()] {
        let text = text.to_owned();
        let findings = within(std::time::Duration::from_secs(1), move || {
            run(".github/workflows/ci.yml", &text, Some("github_actions"))
        });
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(
            findings[0].analysis_failure_kind(),
            Some(AnalysisFailureKind::ResourceParse)
        );
    }
}

/// A document over 1 MiB takes its share of the YAML parse budget while it
/// parses, as every parse of repository YAML does (ADR-0042); a smaller one
/// takes none.
#[test]
fn a_large_document_takes_the_yaml_parse_budget() {
    use alint_core::yaml_budget::{YAML_BUDGET_THRESHOLD_BYTES, reservations_on_this_thread};
    use std::fmt::Write as _;
    let mut large = String::from("jobs:\n");
    for job in 0.. {
        if large.len() > YAML_BUDGET_THRESHOLD_BYTES {
            break;
        }
        let _ = writeln!(large, "  job{job}:\n    runs-on: ubuntu-latest");
    }
    let before = reservations_on_this_thread();
    assert!(YamlDocument::parse("jobs: {}\n").unwrap().is_some());
    assert_eq!(reservations_on_this_thread(), before);
    assert!(YamlDocument::parse(&large).unwrap().is_some());
    assert_eq!(reservations_on_this_thread(), before + 1);
}

/// Without the guard, the lookups bound themselves: an alias to a collection
/// still open is refused while parsing, and merge work runs out of budget (a
/// small one here, to keep a debug build fast: the production one is spent in
/// about 0.1 s in a release build).
#[test]
fn merge_lookups_are_bounded_without_the_guard() {
    for cycle in ["&r\n<<: [*r, *r]\n", "jobs:\n  a: &r\n    <<: [*r, *r]\n"] {
        let error = YamlDocument::parse_unguarded(cycle).unwrap_err();
        assert!(error.contains("contains it"), "{error}");
    }
    let chain = merge_chain("", 12, 10);
    let document = YamlDocument::parse_unguarded(&chain).unwrap().unwrap();
    document.merge_work.set(Some(MAX_MERGE_WORK / 32));
    let exhausted = within(std::time::Duration::from_secs(1), move || {
        let rule = github_rule();
        let mut collector = Collector::new(&rule, Path::new("ci.yml"), &document, &chain);
        collect_github_actions(&mut collector);
        document.merge_work.get().is_none()
    });
    assert!(exhausted);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    /// Any merge chain after an ayaml-only error: the rule refuses it, and
    /// read without the guard, its lookups' work counter never passes the
    /// budget, and the lookups either finish (and find the reference) or
    /// report the budget spent.
    #[test]
    fn an_ayaml_error_before_merge_aliases_never_runs_unbounded(
        prefix in proptest::sample::select(AYAML_ONLY_ERRORS),
        depth in 2usize..14,
        fanout in 1usize..11,
    ) {
        const BUDGET: usize = 100_000;
        let limit = std::time::Duration::from_secs(5);
        let text = merge_chain(prefix, depth, fanout);
        let shared = text.clone();
        let kinds = within(limit, move || {
            run(".github/workflows/ci.yml", &shared, None)
                .iter()
                .map(Violation::analysis_failure_kind)
                .collect::<Vec<_>>()
        });
        proptest::prop_assert_eq!(kinds, [Some(AnalysisFailureKind::ResourceParse)]);
        let unguarded = within(limit, move || {
            let Ok(Some(document)) = YamlDocument::parse_unguarded(&text) else {
                return None;
            };
            document.merge_work.set(Some(BUDGET));
            let rule = github_rule();
            let mut collector = Collector::new(&rule, Path::new("ci.yml"), &document, &text);
            collect_github_actions(&mut collector);
            Some((document.merge_work.get(), collector.findings.len()))
        });
        match unguarded {
            None => {}
            Some((Some(left), findings)) => {
                proptest::prop_assert!(left <= BUDGET);
                proptest::prop_assert_eq!(findings, 1);
            }
            Some((None, _)) => proptest::prop_assert!(fanout > 1),
        }
    }
}

#[test]
fn gitlab_extends_diamond_dag_completes_linearly() {
    use std::fmt::Write as _;
    // Without memoization, 10 levels x 10 repeated parent references per level
    // would visit 10^10 paths even with zero YAML aliases/merge keys.
    let mut yaml = String::from(".t0:\n  image: alpine:latest\n");
    for level in 1..=10 {
        let prev = format!(".t{}", level - 1);
        let parents = [prev.as_str(); 10].join(", ");
        let _ = writeln!(yaml, ".t{level}:\n  extends: [{parents}]");
    }
    yaml.push_str("build:\n  extends: .t10\n  script: [echo hi]\n");

    let findings = within(std::time::Duration::from_secs(2), move || {
        run(".gitlab-ci.yml", &yaml, None)
    });
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].analysis_failure_kind(), None);
}

#[test]
fn redefined_anchor_resolves_lexically_and_rejects_shadowed_self_cycle() {
    // YAML 1.2 §7.1: `*step` in job `b` must resolve to job `a`'s unpinned
    // `&step` even when job `c` later redefines `&step` to a pinned action.
    let shadowed = "\
jobs:
  a:
    steps:
      - &step
        uses: actions/checkout@v4
  b:
    steps:
      - *step
  c:
    steps:
      - &step
        uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9b6834
      - *step
";
    let findings = run(".github/workflows/ci.yml", shadowed, None);
    // Both job `a`'s definition and job `b`'s alias use the unpinned action;
    // job `c`'s redefined `&step` and `*step` are pinned.
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(
        findings.iter().all(|f| f.analysis_failure_kind().is_none()),
        "{findings:?}"
    );

    // Shadowing an existing anchor name inside its own still-open collection
    // must still be rejected as a self-cycle in `parse_unguarded`.
    let cyclic = "a: &x { uses: actions/checkout@v4 }\nb: &x { k: *x }\n";
    let err = YamlDocument::parse_unguarded(cyclic).unwrap_err();
    assert!(err.contains("collection that contains it"), "{err}");
}

#[test]
fn dockerfile_chained_onbuild_heredoc_and_case_insensitive_add_url() {
    // Chained ONBUILD is invalid per Dockerfile grammar -> ResourceParse.
    let chained = "FROM alpine@sha256:0000000000000000000000000000000000000000000000000000000000000000\n\
                   ONBUILD ONBUILD FROM ubuntu:latest\n";
    let findings = run("Dockerfile", chained, None);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceParse)
    );

    // ENV `<<EOF` and RUN `<<<` here-string must NOT start heredocs that swallow FROM;
    // RUN `<<\EOF` IS a real heredoc; uppercase `ADD HTTPS://...` must be flagged.
    let df = "FROM alpine@sha256:0000000000000000000000000000000000000000000000000000000000000000\n\
              ENV MSG=\"<<EOF\"\n\
              RUN cat <<<EOF\n\
              RUN <<\\EOF\n\
              FROM fake:latest\n\
              EOF\n\
              ADD HTTPS://example.com/archive.tar.gz /tmp/\n\
              FROM ubuntu:latest\n";
    let findings = run("Dockerfile", df, None);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings.iter().all(|f| f.analysis_failure_kind().is_none()));
    assert!(
        findings.iter().any(|f| {
            f.message.contains("ADD source")
                && f.baseline_key
                    .as_deref()
                    .is_some_and(|k| k.contains("HTTPS://example.com"))
        }),
        "{findings:?}"
    );
    assert!(
        findings.iter().any(|f| {
            f.message.contains("base image")
                && f.baseline_key
                    .as_deref()
                    .is_some_and(|k| k.contains("ubuntu:latest"))
        }),
        "{findings:?}"
    );
}

#[test]
fn yaml_non_scalar_mapping_keys_and_deep_merge_fail_closed() {
    // Non-scalar YAML mapping key must not panic or skip later valid keys.
    let yaml =
        "jobs:\n  [a, b]: ignored\n  build:\n    steps:\n      - uses: actions/checkout@v4\n";
    let findings = run(".github/workflows/ci.yml", yaml, None);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].analysis_failure_kind(), None);
}
