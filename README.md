# php-lsp

A PHP language server written in pure Rust. Goal: every Intelephense feature (premium ones
included), completely free, plus extras like refactorings, framework support and tool
integration.

It can be used in three ways:

1. **As a Rust library.** `php-ide` gives you a plain Rust API with no JSON-RPC involved.
2. **As a standalone language server.** The `php-lsp` binary talks LSP over stdio or a socket.
3. **As a drop-in replacement for Intelephense.** It accepts the same CLI flags and reads the
   `intelephense.*` settings, so existing editor configurations keep working.

## Crates

| Crate        | Purpose |
|--------------|---------|
| `php-syntax` | Lossless, error tolerant lexer + parser (rowan CST) for PHP 5.3 – 8.5, typed AST, version gating |
| `php-ide`    | IDE features as a Rust API (`Workspace`): diagnostics, document symbols, folding, selection ranges |
| `php-lsp`    | LSP server and `php-lsp` binary |

## Usage

```sh
cargo build --release
./target/release/php-lsp --stdio          # or --socket=<port>
```

To use it as a library:

```rust
use php_ide::{Config, Workspace};

let mut ws = Workspace::new(Config::default());
let file = ws.set_file("file:///a.php", "<?php class A { function f() {} }".into());
for symbol in ws.document_symbols(file) {
    println!("{} {:?}", symbol.name, symbol.kind);
}
```

To embed the LSP server, pass any `lsp_server::Connection` to `php_lsp::Server::initialize`.
This works with stdio, TCP or an in-memory connection.

### Settings

The server reads settings from `initializationOptions`, `workspace/didChangeConfiguration` and
`workspace/configuration`. Both the `intelephense` and the `php-lsp` sections are supported;
when both set the same key, `php-lsp` wins.

| Setting                   | Default   |
|---------------------------|-----------|
| `environment.phpVersion`  | `8.4`     |
| `diagnostics.enable`      | `true`    |
| `files.maxSize`           | `1000000` |

## Status / roadmap

- [x] **Phase 1, foundation:** parser for all PHP versions, syntax diagnostics, document
  symbols, folding, selection ranges, LSP server (stdio/socket, incremental sync, UTF-8/16/32
  positions)
- [ ] **Phase 2, semantics & index:** name resolution, PHPDoc, workspace index + cache,
  phpstorm-stubs, Composer/PSR-4, hover, definition, references, workspace symbols,
  completion, signature help
- [ ] **Phase 3, types & diagnostics:** type inference (generics, `@template`, array shapes),
  undefined symbol / type diagnostics, inlay hints, semantic tokens, highlights
- [ ] **Phase 4, editing:** rename, code actions, formatter, implementations,
  call/type hierarchy, code lens, PHPDoc generation
- [ ] **Phase 5, Intelephense compatibility:** full settings schema, `--node-ipc`, custom
  messages, `intelephense` alias binary
- [ ] **Phase 6, extras:** refactorings, dead code / unused detection, PHPStan / Psalm /
  PHPCS / php-cs-fixer / PHPUnit integration
- [ ] **Phase 7, frameworks:** plugin API, Laravel, Symfony, WordPress

## Development

```sh
cargo test --workspace
cargo run --release -p php-syntax --example parse_corpus -- path/to/php/project
cargo run -p php-syntax --example fuzz_truncate -- some/file.php
```
