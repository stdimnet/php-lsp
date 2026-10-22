# Installation

## Release downloads

Every release on GitHub (tags `v*`) has these archives, plus a
`SHA256SUMS` file:

| Archive | Contents |
|---|---|
| `php-lsp-<version>-<target>.tar.gz` / `.zip` | the `php-lsp` binary, the Node.js launcher (`node/`), README, licenses |
| `libphplsp-<version>-<target>.tar.gz` / `.zip` | the C library: `include/php_lsp.h`, `lib/` (shared and static library), `bindings/` (C example, Python binding) |
| `php-lsp-wasm-<version>.tgz` | the npm package (also published to npm as `php-lsp-wasm`) |

Targets:

| Target | Server binary | C library |
|---|---|---|
| `x86_64-unknown-linux-gnu` | yes (glibc 2.35+) | `libphplsp.so`, `libphplsp.a` |
| `aarch64-unknown-linux-gnu` | yes | `libphplsp.so`, `libphplsp.a` |
| `x86_64-unknown-linux-musl` | yes, fully static (any Linux) | no |
| `aarch64-unknown-linux-musl` | yes, fully static | no |
| `x86_64-apple-darwin` | yes | `libphplsp.dylib`, `libphplsp.a` |
| `aarch64-apple-darwin` | yes | `libphplsp.dylib`, `libphplsp.a` |
| `x86_64-pc-windows-msvc` | `php-lsp.exe` | `phplsp.dll`, `phplsp.dll.lib`, `phplsp.lib` |
| `aarch64-pc-windows-msvc` | `php-lsp.exe` | `phplsp.dll`, `phplsp.dll.lib`, `phplsp.lib` |

Unpack the server archive and put `php-lsp` (or `php-lsp.exe`) on your
`PATH`, then [set up your editor](editors.md). Check it with:

```sh
php-lsp --version
```

The binary has no runtime dependencies: PHP itself is not needed (the PHP
standard library is described by the embedded phpstorm-stubs). PHP is only
needed for the optional external tools (PHPStan, Psalm, PHPCS,
php-cs-fixer, PHPUnit).

## From source

Requirements: Rust 1.82 or newer (`rustup`), a C compiler only for the C
library examples.

```sh
git clone https://github.com/stdimnet/php-lsp
cd php-lsp
cargo build --release -p php-lsp          # target/release/php-lsp
cargo build --release -p php-lsp-ffi      # target/release/libphplsp.{so,dylib,a} / phplsp.dll
```

Or install the binary into `~/.cargo/bin`:

```sh
cargo install --git https://github.com/stdimnet/php-lsp php-lsp
```

The WebAssembly package needs the `wasm32-unknown-unknown` target and
`wasm-bindgen-cli` 0.2.100:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.100
crates/php-lsp-wasm/build.sh              # crates/php-lsp-wasm/pkg
```

## npm

```sh
npm install php-lsp-wasm
```

See [WebAssembly](library-wasm.md).

## As a Rust dependency

```toml
[dependencies]
php-ide = { git = "https://github.com/stdimnet/php-lsp", features = ["parallel"] }
# or the whole server, embeddable:
php-lsp = { git = "https://github.com/stdimnet/php-lsp" }
```

See [Rust library](library-rust.md).
