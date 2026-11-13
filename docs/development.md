# Development

Requirements: Rust 1.82+; Node.js 18+ for the comparison, the VS Code test
and the WebAssembly test; a C compiler and Python 3 for the C library
examples.

## Build and test

```sh
cargo build --release                      # all crates, target/release/php-lsp
cargo test --workspace                     # unit, integration and doc tests
cargo fmt --all
cargo clippy --workspace --all-targets
cargo clippy -p php-lsp-wasm --target wasm32-unknown-unknown
```

CI (`.github/workflows/ci.yml`) runs exactly these with `-D warnings`, the
tests on Linux and macOS, a Windows build, the C example, the Python binding
and the WebAssembly build with its Node test.

## Corpus checks

Run against real projects (Laravel, Symfony, WordPress, PHP-Parser, ...):

```sh
# parser: every file parses, the tree reproduces the text
cargo run --release -p php-syntax --example parse_corpus -- path/to/project
# parser and IDE features on every truncated prefix of a file (no panics)
cargo run -p php-syntax --example fuzz_truncate -- some/file.php
cargo run --release -p php-ide --example fuzz_ide -- some/*.php
# formatter: only whitespace changes, idempotent, inline HTML text kept
cargo run --release -p php-fmt --example fmt_corpus -- path/to/project
# rename every symbol, check for new diagnostics and the exact round trip
cargo run --release -p php-ide --example rename_check -- path/to/project
# diagnostics of a whole project (optionally only some codes)
cargo run --release -p php-ide --example diagnose_project -- path/to/project [P1013,P1014]
# index a project, look up a class
cargo run --release -p php-ide --example index_project -- path/to/project ClassName
# type / hover / definition at a position (1-based)
cargo run --release -p php-ide --example query -- path/to/project src/A.php 10 5
```

Small tools: `php-syntax --example tree -- file.php` (syntax tree),
`php-fmt --example fmt_file -- file.php` (formatted output),
`php-fmt --example beautify -- html|html-only|js|css <out dir> <files>`
(the js-beautify port alone, for comparisons with js-beautify).
`fmt_corpus` prints failing files with `VERBOSE=1`.

## Comparison with Intelephense

`editors/compare/compare.js` runs php-lsp and Intelephense side by side
(see its [README](../editors/compare/README.md)):

```sh
cargo build --release -p php-lsp
node editors/vscode-test/run.js            # once: downloads the extension (contains Intelephense)
node editors/compare/compare.js path/to/project --files 300 --out report.json
REFACTOR=1 node editors/compare/compare.js path/to/project   # formatting, references, rename, code actions
```

Useful variables: `PHPV=8.3.0`, `EXACT=1` (ranges and messages),
`FMTTABS=1` (format with tabs), `FORMATDUMP=dir` (write both versions of
every file formatted differently).

## VS Code

`editors/vscode-test` installs the Intelephense extension into a test
VS Code, swaps its server for php-lsp (`editors/node/php-lsp.js`) and runs
the checks in `suite/` (diagnostics, hover, completion, definition,
formatting, rename, ...):

```sh
cd editors/vscode-test && npm install
xvfb-run -a node run.js      # xvfb-run only without a display
```

## Libraries

```sh
cargo build --release -p php-lsp-ffi
cd crates/php-lsp-ffi/bindings/c && cc example.c -I../../include -L../../../../target/release -lphplsp -o example \
  && LD_LIBRARY_PATH=../../../../target/release ./example
PHP_LSP_LIB=$PWD/target/release/libphplsp.so python3 crates/php-lsp-ffi/bindings/python/php_lsp.py

crates/php-lsp-wasm/build.sh && node crates/php-lsp-wasm/tests/node.mjs
```

The C header `crates/php-lsp-ffi/include/php_lsp.h` is written by hand;
change it together with `crates/php-lsp-ffi/src/lib.rs`.

## Adding a diagnostic

1. Add a variant to `DiagnosticCode` (`crates/php-ide/src/diagnostics.rs`)
   with Intelephense's code in `as_str`, or use
   `DiagnosticCode::Catalog("P1xxx")`; use Intelephense's message and
   severity.
2. Implement the check in `diagnostics.rs`, `lint.rs`, `lint_types.rs`,
   `lint_compat.rs` or `typecheck.rs`, guarded by the matching
   `DiagnosticsConfig` group.
3. Add cases to `crates/php-ide/tests/ide.rs`, and compare with
   Intelephense on a corpus (`EXACT=1`).
4. Add the code to the table in `docs/diagnostics.md`.

## Releases

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`) and run
   `cargo build` to update `Cargo.lock`.
2. Commit, tag `vX.Y.Z`, push the tag.
3. `.github/workflows/release.yml` builds the server and the C library for
   every target, the npm package, and publishes a GitHub release with
   `SHA256SUMS`. The npm package is published when the repository secret
   `NPM_TOKEN` is set.

A manual run (`workflow_dispatch`) builds everything and keeps the archives
as workflow artifacts without releasing.

## Licences

php-lsp is MIT OR Apache-2.0. Bundled third-party code (phpstorm-stubs,
the js-beautify port) is listed in `THIRD_PARTY_NOTICES.md`.
