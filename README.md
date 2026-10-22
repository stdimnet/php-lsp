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
| `diagnostics.*`           | `undefinedTypes`, `undefinedFunctions`, `undefinedConstants`, `undefinedClassConstants`, `undefinedMethods`, `undefinedProperties`, `undefinedVariables`, `unusedSymbols`, `deprecated`, `argumentCount`, `implementationErrors`, `typeErrors`, `unreachableCode`, `languageConstraints`, `memberAccess`, `suspectCode`, `duplicateSymbols` (all `true`; `"on"` / `"off"` also accepted) |
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
| `index.cache`                             | `true`: symbols are kept on disk (in `storagePath`, else `~/.cache/php-lsp`), so a restart only parses changed files. "Index workspace" and `clearCache` rebuild it |

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
  signature help, file watching, Composer/PSR-4, on-disk index cache (12.6k files: 0.85 s
  cold, 0.2 s with the cache)
- [x] **Phase 3, types & diagnostics:** type inference (generics via `@template`, array
  shapes, narrowing), inlay hints, semantic tokens, highlights. Diagnostics use
  Intelephense's codes, messages and severities: undefined symbols, argument count and
  types (P1006), implementations, deprecations, unused symbols and parameters, missing
  returns, possibly undefined variables, unreachable code, member visibility, modifier /
  method body / readonly / named argument rules, duplicate keys and declarations,
  incompatible / final overrides, colliding traits, attribute targets, enum rules, property
  hooks and asymmetric visibility, constant expressions, namespace placement, deprecations
  depending on the targeted PHP version and more: 112 of the 130 codes of Intelephense's catalog,
  compared with the real Intelephense for PHP 7.4 to 8.5 (`editors/compare`)
- [x] **Phase 4, editing:** rename, code actions (import, implement methods, remove unused
  imports, PHPDoc, getters/setters, sort imports), formatter, implementations, call/type
  hierarchy, code lens, PHPDoc completion, document links (`include`/`require`, URLs),
  file rename → class rename + PSR-4 namespace update (`workspace/willRenameFiles`),
  HTML in templates (tag / attribute / closing tag completion, linked editing of tag names)
- [x] **Phase 5, Intelephense compatibility:** `intelephense.*` settings, `--node-ipc`,
  `cancelIndexing`, `indexingStarted/Ended`, initialization options, Node.js launcher,
  tested inside the real Intelephense VS Code extension (`editors/vscode-test`)
- [x] **Phase 6, extras:** extract variable / method / constant, inline variable,
  constructor promotion, generate constructor, add return type, PSR-4 namespace fix,
  unused detection, PHPStan / Psalm / PHPCS diagnostics, php-cs-fixer / phpcbf formatting,
  PHPUnit code lenses
- [x] **Phase 7, frameworks:** plugin API (`php_ide::plugins::Plugin`, register your own with
  `Workspace::register_plugin`) with built-in plugins:
  - **Laravel:** `app()` / `resolve()` / `make()` container types, Eloquent relations as
    properties (`$user->posts` → `Collection<int, Post>`), completion + go to definition for
    `config()`, `view()`, `route()`, `__()` / `trans()` and `env()`
  - **Blade:** templates are analysed as PHP (echoes, directives, `@php`, `@use`, component
    attributes): hover, completion, definition, references, rename and diagnostics in
    `{{ }}` / `@if(...)` / `@foreach(...)`; variable types come from the controllers' `view()`
    / `->with()` / `compact()` calls, `$loop` is typed; directive snippets, HTML completion,
    view names in `@include` / `@extends`; folding of directive blocks, outline of sections /
    stacks / slots, highlighted directives (semantic tokens), formatting of `@php` blocks
  - **Twig** (for editors that send `.twig` files; the Intelephense VS Code extension does
    not): variable types from the controllers' `render()` / `renderView()` calls and
    `#[Template]`, `{% set %}` and `{% for %}`, `loop`, `app`; hover, completion and go to
    definition of attributes (`user.name` → `getName()`), tags (with the matching end tag),
    filters, functions, tests, template names and routes; go to the template of
    `extends` / `include` / `embed` (also `@Bundle/...`), to the parent's block and to the
    controller passing a variable; document links, folding, outline of blocks and macros,
    semantic tokens. No diagnostics and no formatting for Twig
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
cargo run --release -p php-ide --example diagnose_project -- path/to/php/project [CODE,CODE]
cargo run --release -p php-ide --example fuzz_ide -- some/*.php   # IDE features on truncated files
cargo run --release -p php-fmt --example fmt_corpus -- path/to/php/project
node editors/compare/compare.js path/to/php/project              # differences to Intelephense
xvfb-run -a node editors/vscode-test/run.js                      # inside the real VS Code
```
