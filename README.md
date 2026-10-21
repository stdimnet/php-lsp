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
| `php-hir`    | Semantic model: namespaces/imports, PHPDoc (incl. PHPStan/Psalm types), type model, declared symbols |
| `php-index`  | Global symbol index with inheritance aware member lookup (traits, parents, interfaces, mixins) |
| `php-stubs`  | JetBrains phpstorm-stubs (Apache-2.0) embedded in compressed form |
| `php-fmt`    | Whitespace-only formatter (PSR-12 / PER), never changes tokens, idempotent |
| `php-tools`  | External tools: PHPStan, Psalm, PHP_CodeSniffer, php-cs-fixer, phpcbf, PHPUnit |
| `php-ide`    | IDE features as a Rust API (`Workspace`): completion, hover, navigation, references, symbols, ... |
| `php-lsp`    | LSP server and `php-lsp` binary |

## Usage

```sh
cargo build --release
./target/release/php-lsp --stdio          # or --socket=<port>
```

`--node-ipc` is supported as well, and [`editors/node/php-lsp.js`](editors/node) is a
Node.js launcher for clients that expect a Node server module, such as the Intelephense
VS Code extension. Neovim example:

```lua
vim.lsp.config('php_lsp', { cmd = { 'php-lsp', '--stdio' }, filetypes = { 'php' }, root_markers = { 'composer.json', '.git' } })
vim.lsp.enable('php_lsp')
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
| `files.associations`      | `["*.php", "*.phtml"]` |
| `files.exclude`           | Intelephense defaults (`**/.git/**`, `**/node_modules/**`, ...) |
| `stubs`                   | Intelephense default stub list |
| `completion.insertUseDeclaration` | `true` |
| `completion.maxItems`     | `100` |
| `diagnostics.*`           | `undefinedTypes`, `undefinedFunctions`, `undefinedConstants`, `undefinedClassConstants`, `undefinedMethods`, `undefinedProperties`, `undefinedVariables`, `unusedSymbols`, `deprecated`, `argumentCount`, `implementationErrors` (all `true`) |
| `inlayHint.*`             | `parameterNames`, `returnTypes`, `parameterTypes` (all `true`) |
| `format.enable`           | `true` |
| `format.braces`           | `psr12` (`allman`, `k&r`) |
| `diagnostics.run`         | `onType` (`onSave`) |
| `environment.includePaths`| `[]` (extra directories indexed like `vendor/`) |

php-lsp specific settings (`php-lsp` section only):

| Setting                                   | Default |
|-------------------------------------------|---------|
| `tools.phpstan.enable` / `.path` / `.args`| `false` / `vendor/bin/phpstan` or `$PATH` / `[]` |
| `tools.psalm.*`, `tools.phpcs.*`          | same as above |
| `tools.phpCsFixer.*`, `tools.phpcbf.*`, `tools.phpunit.*` | path / args |
| `format.provider`                         | `builtin` (`php-cs-fixer`, `phpcbf`) |

PHPStan / Psalm / PHPCS run in the background when a file is opened or saved and their
results are merged with php-lsp's own diagnostics. Test classes get "▶ Run test" code lenses
(command `php-lsp.runTest`).

Intelephense's `initializationOptions` (`storagePath`, `globalStoragePath`, `clearCache`,
`licenceKey`, `isVscode`) are accepted; a licence key is not needed for any feature. The
`cancelIndexing` request is supported.

## Status / roadmap

- [x] **Phase 1, foundation:** parser for all PHP versions, syntax diagnostics, document
  symbols, folding, selection ranges, LSP server (stdio/socket, incremental sync, UTF-8/16/32
  positions)
- [x] **Phase 2, semantics & index:** name resolution, PHPDoc, background workspace
  indexing, phpstorm-stubs, hover, definition / type definition / implementation,
  references, document highlights, workspace symbols, completion with auto import,
  signature help, file watching, Composer/PSR-4 (12.5k files index in ~1.3 s, so there is
  no on-disk cache)
- [x] **Phase 3, types & diagnostics:** type inference (generics via `@template`, array
  shapes, narrowing), undefined symbol / argument count / implementation / deprecation /
  unused diagnostics, inlay hints, semantic tokens, highlights
- [x] **Phase 4, editing:** rename, code actions (import, implement methods, remove unused
  imports, PHPDoc, getters/setters, sort imports), formatter, implementations, call/type
  hierarchy, code lens, PHPDoc completion, document links (`include`/`require`, URLs),
  file rename → class rename + PSR-4 namespace update (`workspace/willRenameFiles`),
  HTML in templates (tag / attribute / closing tag completion, linked editing of tag names)
- [x] **Phase 5, Intelephense compatibility:** `intelephense.*` settings, `--node-ipc`,
  `cancelIndexing`, `indexingStarted/Ended`, initialization options, Node.js launcher
- [x] **Phase 6, extras:** extract variable / method / constant, inline variable,
  constructor promotion, generate constructor, add return type, PSR-4 namespace fix,
  unused detection, PHPStan / Psalm / PHPCS diagnostics, php-cs-fixer / phpcbf formatting,
  PHPUnit code lenses
- [x] **Phase 7, frameworks:** plugin API (`php_ide::plugins::Plugin`, register your own with
  `Workspace::register_plugin`) with built-in plugins:
  - **Laravel:** `app()` / `resolve()` / `make()` container types, Eloquent relations as
    properties (`$user->posts` → `Collection<int, Post>`), completion + go to definition for
    `config()`, `view()`, `route()`, `__()` / `trans()` and `env()`
  - **Symfony:** typed `$container->get(Foo::class)` and Doctrine `getRepository()`, route
    names (`#[Route(name: ...)]`, YAML) and Twig templates for `redirectToRoute()`,
    `generateUrl()`, `render()`
  - **WordPress:** action / filter hook names for `add_action()`, `add_filter()`, ... from
    `do_action()` / `apply_filters()` calls, with go to definition

## Development

```sh
cargo test --workspace
cargo run --release -p php-syntax --example parse_corpus -- path/to/php/project
cargo run -p php-syntax --example fuzz_truncate -- some/file.php
cargo run --release -p php-ide --example index_project -- path/to/php/project ClassName
cargo run --release -p php-ide --example diagnose_project -- path/to/php/project [CODE]
cargo run --release -p php-fmt --example fmt_corpus -- path/to/php/project
```
