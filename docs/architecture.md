# Architecture

## Crates

```text
php-syntax, php-hir, php-index, php-fmt, php-stubs → php-ide
php-ide, php-tools                                  → php-lsp
php-lsp                                             → php-lsp-ffi (C library), php-lsp-wasm (WebAssembly)
```

| Crate | Responsibility |
|---|---|
| `php-syntax` | Lexer and parser for PHP 5.3 to 8.5. Lossless (every byte, whitespace and comment is in the tree), error tolerant (always produces a tree), rowan CST with a typed AST layer, version gating (`PhpVersion`) |
| `php-hir` | Semantic model of one file: namespaces and imports, name resolution, PHPDoc parsing (PHPStan / Psalm types), the `Type` model, declared symbols |
| `php-index` | Global symbol index over all files (`FileId`), inheritance aware member lookup through parents, interfaces, traits and `@mixin` |
| `php-stubs` | phpstorm-stubs, packed and deflate-compressed by `build.rs`, unpacked on demand per extension |
| `php-fmt` | Formatter: PHP (whitespace-only edits on the token stream) and the js-beautify port for HTML / CSS / JavaScript in templates (`php_fmt::beautify`) |
| `php-tools` | Runs PHPStan, Psalm, PHPCS, php-cs-fixer, phpcbf and PHPUnit and parses their output |
| `php-ide` | `Workspace` and every IDE feature: inference, diagnostics, completion, navigation, refactorings, Blade / Twig, plugins |
| `php-lsp` | LSP server: configuration, protocol conversion, indexing jobs, file watching, index cache, tool integration; the `php-lsp` binary |
| `php-lsp-ffi` | C ABI over `php_lsp::Embedded` (`libphplsp`) |
| `php-lsp-wasm` | wasm-bindgen API over `php_lsp::Embedded` (`php-lsp-wasm` on npm) |

## Data flow

1. **Startup.** `initialize` creates the `Workspace`; after `initialized`
   the server reads the settings and starts indexing in the background:
   the workspace roots (`collect_files` with `files.*`), Composer's
   autoload data for `vendor/`, include paths and the stubs. Files are
   parsed in parallel; only their declarations go into the index. With
   `index.cache`, unchanged files come from the cache. During indexing,
   diagnostics about undefined symbols are held back.
2. **Open documents** are kept fully parsed (`set_file` on every change,
   incremental changes are applied to the text first). Other files are
   parsed on demand when a feature needs their bodies (e.g. references).
3. **A request** (say hover) is converted from LSP positions to a byte
   offset (`LineIndex`, UTF-16 / UTF-8 / UTF-32), handled by a
   `Workspace` method and converted back. Features work on the CST of the
   file, ask `php-hir` for names and PHPDoc, the type inference
   (`infer.rs`) for expression types and `php-index` for declarations.
   Plugins are consulted by the inference and by string completion.
4. **Diagnostics** are computed after changes (or on save with
   `diagnostics.run: onSave`) for open files and published; external tools
   run in background threads and their results are merged.

## Templates

- **Inline HTML** in `.php` files is a token of the PHP tree; HTML features
  (`html.rs`) work on that text.
- **Blade** (`blade.rs`) is translated into virtual PHP code with the same
  byte offsets: HTML becomes whitespace, `{{ e }}` becomes `;(e);`,
  directives become the matching PHP statements. Every PHP feature then
  works on Blade without offset mapping.
- **Twig** (`twig.rs`) has its own small parser; types come from the
  controllers that render a template.

## Formatter

The PHP formatter never re-prints code: it walks the tokens, decides the
whitespace between each pair (indentation from the tree, Intelephense's
spacing and line break rules) and emits edits for the whitespace that
differs. Templates first go through the HTML beautifier (a port of
js-beautify with Intelephense's patches, PHP regions as placeholders that
carry their block depth), then the PHP pass aligns each region to its
`<?php` tag. See [formatting](formatting.md).

## Tests

Unit tests next to the code, integration tests in `crates/*/tests`
(`php-syntax/tests/parser.rs`, `php-ide/tests/ide.rs` for features,
`php-lsp/tests/e2e.rs` for the protocol, `php-lsp-wasm/tests/node.mjs`),
doc tests, corpus checks (parse every file of real projects, format them
idempotently, rename round trips), the comparison with Intelephense and a
run inside VS Code. See [development](development.md).
