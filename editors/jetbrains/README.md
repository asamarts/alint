# alint for JetBrains IDEs

In-editor diagnostics, hover-to-explain, and quick-fixes for
[alint](https://github.com/asamarts/alint) across the JetBrains suite
(IntelliJ IDEA, PyCharm, GoLand, WebStorm, RustRover, CLion, Rider,
Android Studio).

The plugin is glue: it registers `alint lsp` (the alint language
server) with [LSP4IJ](https://github.com/redhat-developer/lsp4ij) and
lets the IDE render the results. All linting logic lives in alint.

## How the binary is found

1. the path set in **Settings → … → alint** (`alint.path` equivalent),
2. `alint` on your `PATH`,
3. a copy the plugin previously downloaded,
4. otherwise a notification offers to download the matching release
   (opt-in, SHA-256 verified) — or install it yourself via Homebrew,
   `cargo install alint`, `npm i -g @asamarts/alint`, Docker, or the
   install script.

## Building locally

The plugin builds cleanly with the committed Gradle wrapper and a
**full JDK 21** (not a JRE — code compilation needs `javac`):

```sh
cd editors/jetbrains
./gradlew buildPlugin     # produces build/distributions/alint-jetbrains-*.zip
./gradlew runIde          # launch a sandbox IDE with the plugin
```

The build is exercised by the `editors` CI job (it also typechecks the
VS Code extension and wasm-builds the Zed one). A headless `runIde`
smoke has confirmed the plugin loads in a real IntelliJ 2024.2 alongside
LSP4IJ with no errors; what's left to verify by hand is the server
actually attaching and surfacing diagnostics when you open a file in a
project (`./gradlew runIde`, then open a repo with a `.alint.yml`).

## Publishing

The `publish-jetbrains` job in the repo's `release.yml` runs
`gradle publishPlugin` on a version tag, gated on the
`JETBRAINS_MARKETPLACE_TOKEN` secret (plus signing secrets). The
`asamarts` Marketplace vendor must exist first.

## Versioning

The plugin version tracks the alint release it ships against.
