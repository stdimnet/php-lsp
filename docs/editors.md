# Editors

php-lsp speaks the Language Server Protocol over stdio by default, so every
editor with an LSP client can use it. The command is always:

```sh
php-lsp --stdio
```

It also accepts Intelephense's other transports (`--socket=<port>`,
`--node-ipc`) and reads Intelephense's settings (`intelephense.*`), so
configurations written for Intelephense keep working. Settings go into the
`intelephense` or the `php-lsp` section (see [configuration](configuration.md)).

## VS Code

php-lsp replaces the server inside the official Intelephense extension; the
extension's UI, commands and settings stay the same and every premium
feature is available without a licence key.

1. Install the Intelephense extension (`bmewburn.vscode-intelephense-client`).
2. Put the `php-lsp` binary somewhere, e.g. `~/.local/bin/php-lsp`.
3. In the extension's install directory
   (`~/.vscode/extensions/bmewburn.vscode-intelephense-client-<version>/`),
   replace `node_modules/intelephense/lib/intelephense.js` with
   `node/php-lsp.js` from the release archive (or `editors/node/php-lsp.js`
   from the repository).
4. Tell the launcher where the binary is: either set the environment
   variable `PHP_LSP_BIN=/path/to/php-lsp`, put the binary at
   `node_modules/intelephense/lib/bin/php-lsp` (next to the launcher), or
   have `php-lsp` on the `PATH` VS Code sees.
5. Reload the window. "PHP language server" output shows `php-lsp`.

The launcher bridges the extension's Node IPC channel to `php-lsp
--node-ipc`. Updating the extension restores the original file; repeat
step 3 afterwards. `editors/vscode-test` checks this setup in a real VS Code
(see [development](development.md#vs-code)).

## Neovim

Neovim 0.11 and later:

```lua
vim.lsp.config('php_lsp', {
  cmd = { 'php-lsp', '--stdio' },
  filetypes = { 'php', 'blade' },
  root_markers = { 'composer.json', '.git' },
  settings = {
    intelephense = { environment = { phpVersion = '8.3' } },
  },
})
vim.lsp.enable('php_lsp')
```

With nvim-lspconfig's Intelephense configuration, only the command changes:

```lua
require('lspconfig').intelephense.setup({ cmd = { 'php-lsp', '--stdio' } })
```

## Helix

`~/.config/helix/languages.toml`:

```toml
[language-server.php-lsp]
command = "php-lsp"
args = ["--stdio"]

[language-server.php-lsp.config.intelephense]
environment.phpVersion = "8.3"

[[language]]
name = "php"
language-servers = ["php-lsp"]

[[language]]
name = "blade"
language-servers = ["php-lsp"]
```

## Zed

Zed's PHP extension starts Intelephense; point it at php-lsp in
`settings.json`:

```json
{
  "lsp": {
    "intelephense": {
      "binary": { "path": "/path/to/php-lsp", "arguments": ["--stdio"] }
    }
  },
  "languages": {
    "PHP": { "language_servers": ["intelephense", "!phpactor", "..."] }
  }
}
```

## Sublime Text

With the LSP package, `Preferences > Package Settings > LSP > Settings`:

```json
{
  "clients": {
    "php-lsp": {
      "enabled": true,
      "command": ["php-lsp", "--stdio"],
      "selector": "source.php | embedding.php | text.html.blade"
    }
  }
}
```

## Emacs

eglot:

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '((php-mode php-ts-mode web-mode) . ("php-lsp" "--stdio"))))
```

lsp-mode (reuses its Intelephense client):

```elisp
(setq lsp-intelephense-server-command '("php-lsp" "--stdio"))
```

## JetBrains IDEs

PhpStorm has its own PHP support. For other JetBrains IDEs, install the
LSP4IJ plugin, then `Settings > Languages & Frameworks > Language Servers >
+`: command `php-lsp --stdio`, mapping: file type "PHP" (and "Blade").

## Kate

`Settings > Configure Kate > LSP Client > User Server Settings`:

```json
{
  "servers": {
    "php": {
      "command": ["php-lsp", "--stdio"],
      "rootIndicationFileNames": ["composer.json"],
      "highlightingModeRegex": "^PHP.*$"
    }
  }
}
```

## Other clients

Any LSP client works with `php-lsp --stdio`. Clients that expect a Node.js
module can run `node php-lsp.js --stdio` (or `--node-ipc`) from the
release's `node/` directory. In-process use without a separate binary is
possible with the [C library](library-c.md), the
[WebAssembly package](library-wasm.md) or the [Rust crates](library-rust.md).
