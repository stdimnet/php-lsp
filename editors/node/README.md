# php-lsp Node.js launcher

`php-lsp.js` starts the native `php-lsp` binary and forwards LSP messages. It
lets editors / extensions that expect a Node.js server module use php-lsp:

- **Node IPC** (`vscode-languageclient` with `TransportKind.ipc`): run as
  `node php-lsp.js --node-ipc`; messages are bridged to `php-lsp --node-ipc`.
- **stdio / socket**: `node php-lsp.js --stdio` or `--socket=<port>` simply
  execs the binary.

The binary is taken from `$PHP_LSP_BIN`, `./bin/php-lsp` next to the script,
or `php-lsp` on `$PATH`.

## Using it with the Intelephense VS Code extension

The extension starts `node_modules/intelephense/lib/intelephense.js` inside
its install directory with `TransportKind.ipc`. To run php-lsp instead,
replace that file with `php-lsp.js` (and put the binary next to it in
`bin/`, or set `PHP_LSP_BIN`). php-lsp reads the same `intelephense.*`
settings and accepts the extension's `initializationOptions` and its
`cancelIndexing` request. All features are available without a licence key.
