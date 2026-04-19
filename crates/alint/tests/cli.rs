//! CLI snapshot tests. Each `*.toml` under `tests/cli/` describes
//! one invocation of the `alint` binary, paired with sandbox input
//! (`*.in/`), expected post-run tree (`*.out/`), and expected
//! stdout / stderr / exit status. Run via `cargo test -p alint
//! --test cli`; regenerate expected output with
//! `TRYCMD=overwrite cargo test -p alint --test cli`.

#[test]
fn cli_tests() {
    trycmd::TestCases::new().case("tests/cli/*.toml");
}
