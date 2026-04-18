# alint

A language-agnostic linter for **repository structure, file existence, filename conventions, and file content rules**.

> Status: early development. v0.1 is the MVP — see [docs/design/ROADMAP.md](docs/design/ROADMAP.md) for scope per version.

## What alint does

alint enforces declarative rules over a repository tree. The rule model and DSL are described in full at [docs/design/ARCHITECTURE.md](docs/design/ARCHITECTURE.md). Representative rules (some shipped in v0.1, some planned for v0.2):

- A `README.md` must exist at the repo root. *(v0.1)*
- Every `.java` file must start with a required license-header comment. *(v0.1)*
- Filenames under `components/` must be PascalCase. *(v0.1)*
- No binary files may exist under `src/`. *(v0.1)*
- Every `.c` file must have a matching `.h` file in the same directory. *(v0.2 — cross-file rules)*
- For every subdirectory of `src/`, a `mod.rs` must exist. *(v0.2 — per-directory quantification)*

Rules are defined in `.alint.yml`. alint walks the tree (honoring `.gitignore` by default), matches each rule against the index, and emits results in `human` or `json` format today, with `sarif`, GitHub Actions annotations, JUnit, and Markdown arriving in v0.2–v0.3.

## Non-goals

alint is deliberately **not**:

- a code / AST linter — use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [ruff](https://docs.astral.sh/ruff/)
- a SAST scanner — use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/)
- an IaC scanner — use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/)
- a commit-message linter — use [commitlint](https://commitlint.js.org/)
- a secret scanner — use [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog)

Scope is the filesystem shape and contents of a repository, not the semantics of the code inside it.

## Install

### From a tagged release (recommended)

```bash
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball, verifies the SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). Windows users download the Windows tarball from the [Releases page](https://github.com/asamarts/alint/releases).

### From crates.io

```bash
cargo install alint
```

### From source

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo build --release -p alint
./target/release/alint --help
```

## Quick start

Create a `.alint.yml` at the root of your repository:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
version: 1

rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md", "README"]
    root_only: true
    level: error

  - id: no-large-blobs
    kind: file_max_size
    paths: "**"
    max_bytes: 1048576
    level: warning

  - id: components-pascal
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"
    case: pascal
    level: error

  - id: java-license-header
    kind: file_header
    paths: "**/*.java"
    lines: 20
    pattern: "(?s)Copyright \\(c\\) \\d{4}"
    level: error
```

Then run:

```bash
alint check        # run all rules against the current directory
alint list         # list effective rules
alint explain <id> # show a rule's definition
```

Output formats:

```bash
alint check --format human   # default; colorized for humans
alint check --format json    # stable, versioned JSON schema
```

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default — use `--fail-on-warning` to flip that.

## Docs

- [**ARCHITECTURE.md**](docs/design/ARCHITECTURE.md) — rule model, DSL, execution model, crate layout, plugin model.
- [**ROADMAP.md**](docs/design/ROADMAP.md) — scope per version from v0.1 through v1.0.
- [**docs/benchmarks/METHODOLOGY.md**](docs/benchmarks/METHODOLOGY.md) — how benchmarks are measured and published.
- Per-version, per-platform benchmark results under [`docs/benchmarks/<version>/`](docs/benchmarks/).

## Development

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace        # ~30 tests
cargo run -- check            # dogfood: alint lints itself
cargo bench -p alint-bench    # criterion micro-benches
```

CI is self-hosted with per-job bash scripts under `ci/scripts/` that run locally or in GitHub Actions unchanged. See [ci/env.example](ci/env.example) for runner setup.

## License

Dual-licensed under either of:

- [Apache License 2.0](LICENSE-APACHE) ([SPDX `Apache-2.0`](https://spdx.org/licenses/Apache-2.0.html))
- [MIT License](LICENSE-MIT) ([SPDX `MIT`](https://spdx.org/licenses/MIT.html))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in alint shall be dual-licensed as above, without any additional terms or conditions.
