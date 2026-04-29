//! Structured-query rule throughput.
//!
//! Six rule kinds parse JSON / YAML / TOML on every match and
//! evaluate a JSONPath query against the parsed value:
//!   - `json_path_equals`   / `json_path_matches`
//!   - `yaml_path_equals`   / `yaml_path_matches`
//!   - `toml_path_equals`   / `toml_path_matches`
//! Plus `json_schema_passes` which compiles a JSON Schema once at
//! rule-build time and validates each matched file against it.
//!
//! Bench shape: filesystem-bound (rules read each file fresh) —
//! materialise a tempdir of synthetic configs, then time
//! `Engine::run`. Feature-gated `fs-benches` because tempfile
//! materialisation introduces unavoidable noise.

use std::io::Write;

use alint_core::{Engine, Rule, WalkOptions, walk};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

fn make_json_tree(n_files: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-sq-json-")
        .tempdir()
        .expect("tempdir");
    for i in 0..n_files {
        let path = tmp.path().join(format!("pkg/p{i}/package.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = format!(
            r#"{{"name":"p{i}","version":"1.2.3","scripts":{{"build":"tsc"}},"engines":{{"node":">=18"}}}}"#,
        );
        std::fs::File::create(&path)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    tmp
}

fn make_yaml_tree(n_files: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-sq-yaml-")
        .tempdir()
        .expect("tempdir");
    for i in 0..n_files {
        let path = tmp.path().join(format!(".github/workflows/ci-{i}.yml"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = format!(
            "name: ci-{i}\n\
             permissions:\n  contents: read\n\
             jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n",
        );
        std::fs::File::create(&path)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    tmp
}

fn make_toml_tree(n_files: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-sq-toml-")
        .tempdir()
        .expect("tempdir");
    for i in 0..n_files {
        let path = tmp.path().join(format!("crate/c{i}/Cargo.toml"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body =
            format!("[package]\nname = \"c{i}\"\nedition = \"2024\"\nrust-version = \"1.85\"\n",);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    tmp
}

fn make_schema_tree(n_files: usize) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix("alint-bench-sq-schema-")
        .tempdir()
        .expect("tempdir");
    // One schema covering name + version, one per-file instance.
    let schema = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","required":["name","version"],"properties":{"name":{"type":"string"},"version":{"type":"string"}}}"#;
    let schema_path = tmp.path().join("schemas/package.schema.json");
    std::fs::create_dir_all(schema_path.parent().unwrap()).unwrap();
    std::fs::File::create(&schema_path)
        .unwrap()
        .write_all(schema.as_bytes())
        .unwrap();
    for i in 0..n_files {
        let path = tmp.path().join(format!("pkg/p{i}/package.json"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let body = format!(r#"{{"name":"p{i}","version":"1.0.0"}}"#);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
    }
    tmp
}

fn build_engine(yaml: &str) -> Engine {
    let config = alint_dsl::parse(yaml).expect("config parses");
    let registry = alint_rules::builtin_registry();
    let rules: Vec<Box<dyn Rule>> = config
        .rules
        .iter()
        .map(|spec| registry.build(spec).expect("rule builds"))
        .collect();
    Engine::new(rules, alint_rules::builtin_registry())
}

fn bench_rule(c: &mut Criterion, group: &str, tree: &tempfile::TempDir, yaml: &str, n: u64) {
    let mut g = c.benchmark_group(group);
    let walk_opts = WalkOptions::default();
    let index = walk(tree.path(), &walk_opts).expect("walk");
    let engine = build_engine(yaml);
    g.throughput(Throughput::Elements(n));
    g.bench_with_input(BenchmarkId::from_parameter(n), &index, |b, idx| {
        b.iter(|| engine.run(tree.path(), idx).unwrap());
    });
    g.finish();
}

fn json_path_equals(c: &mut Criterion) {
    for &n in &[100u64, 1000] {
        let tmp = make_json_tree(n as usize);
        bench_rule(
            c,
            "structured_query/json_path_equals",
            &tmp,
            r#"
version: 1
rules:
  - id: scripts-build
    kind: json_path_equals
    paths: "pkg/**/package.json"
    path: "$.scripts.build"
    equals: "tsc"
    level: warning
"#,
            n,
        );
    }
}

fn yaml_path_matches(c: &mut Criterion) {
    for &n in &[100u64, 1000] {
        let tmp = make_yaml_tree(n as usize);
        bench_rule(
            c,
            "structured_query/yaml_path_matches",
            &tmp,
            r#"
version: 1
rules:
  - id: permissions-shape
    kind: yaml_path_matches
    paths: ".github/workflows/*.yml"
    path: "$.permissions['contents']"
    matches: "^read$"
    level: warning
"#,
            n,
        );
    }
}

fn toml_path_equals(c: &mut Criterion) {
    for &n in &[100u64, 1000] {
        let tmp = make_toml_tree(n as usize);
        bench_rule(
            c,
            "structured_query/toml_path_equals",
            &tmp,
            r#"
version: 1
rules:
  - id: cargo-edition
    kind: toml_path_equals
    paths: "crate/**/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: error
"#,
            n,
        );
    }
}

fn json_schema_passes(c: &mut Criterion) {
    for &n in &[100u64, 1000] {
        let tmp = make_schema_tree(n as usize);
        bench_rule(
            c,
            "structured_query/json_schema_passes",
            &tmp,
            r#"
version: 1
rules:
  - id: package-shape
    kind: json_schema_passes
    paths: "pkg/**/package.json"
    schema_path: "schemas/package.schema.json"
    level: error
"#,
            n,
        );
    }
}

criterion_group!(
    benches,
    json_path_equals,
    yaml_path_matches,
    toml_path_equals,
    json_schema_passes,
);
criterion_main!(benches);
