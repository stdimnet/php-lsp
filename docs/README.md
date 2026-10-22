# php-lsp documentation

php-lsp is a PHP language server written in Rust. It offers what Intelephense
offers, the premium features included, without a licence key. You can use it
in four ways:

| As | For | Start here |
|---|---|---|
| A language server binary (`php-lsp`) | any editor with LSP support | [Installation](installation.md), [Editors](editors.md) |
| A drop-in replacement for Intelephense | the Intelephense VS Code extension and its settings | [Editors: VS Code](editors.md#vs-code) |
| A C library (`libphplsp`) | C, C++, Go, Python, C#, Java (JNI / FFM), ... | [C library](library-c.md) |
| A WebAssembly / npm package (`php-lsp-wasm`) | Node.js, browsers, Monaco, web workers | [WebAssembly](library-wasm.md) |
| A Rust library (`php-ide`, `php-lsp`) | Rust programs | [Rust library](library-rust.md) |

## Contents

- [Installation](installation.md): release downloads, building from source.
- [Editors](editors.md): VS Code, Neovim, Helix, Zed, Sublime Text, Emacs, JetBrains IDEs, Kate.
- [Configuration](configuration.md): every setting, initialization options, command line flags, the index cache, external tools.
- [Features](features.md): navigation, completion, type inference, refactorings, frameworks (Laravel, Blade, Symfony, Twig, WordPress).
- [Diagnostics](diagnostics.md): all 131 diagnostic codes.
- [Formatting](formatting.md): the formatter's rules, templates (HTML, CSS, JavaScript), Blade, external formatters.
- Libraries:
  - [C library](library-c.md): the C API, the Python binding and other languages.
  - [WebAssembly](library-wasm.md): the npm package and the JavaScript API.
  - [Rust library](library-rust.md): `php_ide::Workspace`, `php_lsp::Embedded`, plugins.
- [LSP reference](lsp.md): supported requests, custom requests and notifications, commands.
- [Plugins](plugins.md): adding framework support.
- [Architecture](architecture.md): the crates and how a request flows.
- [Development](development.md): tests, comparison with Intelephense, releases.
- [Troubleshooting](troubleshooting.md).
