//! CLI snapshot tests. Each `*.toml` under `tests/cli/` describes
//! one invocation of the `alint` binary, paired with sandbox input
//! (`*.in/`), expected post-run tree (`*.out/`), and expected
//! stdout / stderr / exit status. Run via `cargo test -p alint
//! --test it -- --exact cli::cli_tests`; regenerate expected output with
//! `TRYCMD=overwrite cargo test -p alint --test it -- --exact cli::cli_tests`.

#[test]
fn cli_tests() {
    // Snapshot the normal Unicode presentation independently of the developer's
    // terminal. ASCII fallback itself is covered by GlyphSet unit tests.
    // Every case also runs without the host's Git setup, as every process
    // these tests start does (`crate::hermetic`).
    let cases = trycmd::TestCases::new();
    cases.env("TERM", "xterm-256color");
    for (name, value) in crate::hermetic::environment() {
        cases.env(name, value);
    }
    cases.case("tests/cli/*.toml");
}
