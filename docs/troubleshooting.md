# Troubleshooting

## Where are the logs?

The server logs with `window/logMessage` (fatal startup errors go to
stderr). In VS Code: Output panel, channel of the Intelephense extension.
Neovim: `:LspLog`. Helix: `:log-open`. Indexing ends with a line like
`indexed 1234 files in 850 ms (1200 from cache)`.

## Nothing works / the server does not start

- `php-lsp --version` must run in a terminal. On macOS, a downloaded
  binary may need `xattr -d com.apple.quarantine php-lsp`.
- The editor has to start it with `--stdio` (or the transport it uses).
- VS Code with the Intelephense extension: check that
  `node_modules/intelephense/lib/intelephense.js` is the php-lsp launcher
  (an extension update restores the original) and that the launcher finds
  the binary (`PHP_LSP_BIN`, `lib/bin/php-lsp`, or `PATH`). See
  [editors](editors.md#vs-code).

## "Undefined type / function / method" for things that exist

- Wait until indexing has finished (progress "Indexing PHP files"; these
  diagnostics are held back while it runs).
- The code must be inside a workspace folder, `vendor/` or an
  `environment.includePaths` entry, match `files.associations`, not match
  `files.exclude` and be smaller than `files.maxSize`.
- Built-in extensions need their stub in `stubs` (e.g. `redis`, `imagick`,
  `swoole`, `wordpress`).
- Code generated at runtime (magic methods, facades, containers) needs
  PHPDoc (`@method`, `@property`, `@mixin`) or a [plugin](plugins.md).
  Switch single groups off with `diagnostics.<group>: false`.
- After large changes outside the editor (branch switch, `composer
  install`) without file watching: run "Index workspace"
  (`php-lsp.indexWorkspace`).

## Wrong PHP version errors

Set `environment.phpVersion` (e.g. `"8.1"`). Syntax newer than the target
version is reported (P1022), deprecations depend on it.

## Stale results after a restart

The index cache is keyed by file size and modification time. If it is
ever wrong: "Index workspace", `clearCache: true` in the initialization
options, `index.cache: false`, or delete `~/.cache/php-lsp`.

## Formatting does nothing

- Files with syntax errors are not formatted (see the P1001 diagnostics).
- `format.enable` must be `true`; `format.provider` other than `builtin`
  needs the tool installed (`vendor/bin/php-cs-fixer`, `phpcbf`).
- Templates: when the HTML pass would change text it is skipped; set
  `format.html: false` to format only the PHP parts.

## External tools report nothing

`tools.<tool>.enable` must be `true` in the `php-lsp` section and the tool
findable (`vendor/bin/<tool>`, `tools.<tool>.path` or `PATH`), with PHP on
the `PATH`. Errors of the tool are shown once as a message and in the log.

## High memory or CPU use

Exclude generated code and large data directories with `files.exclude`,
lower `files.maxSize`, or remove unneeded entries from `stubs`. Indexing
uses all cores once at startup; afterwards only changed files are parsed.

## Reporting a bug

Open an issue with the php-lsp version, the editor, the settings, a small
PHP snippet and the log. For a difference to Intelephense, the output of
`editors/compare/compare.js` on the snippet helps.
